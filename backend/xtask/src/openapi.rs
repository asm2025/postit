use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

fn target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../api/openapi.json")
}

fn web_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web")
}

fn schema_path() -> PathBuf {
    web_dir().join("src/api/schema.d.ts")
}

/// Runs the pinned openapi-typescript from `web/node_modules` through `node` (no npx and
/// no `.cmd` shim, so it behaves the same on Windows) and returns its output with LF
/// line endings, so the committed file is byte-stable across platforms.
fn generate_types(spec: &Path) -> anyhow::Result<String> {
    let cli = web_dir().join("node_modules/openapi-typescript/bin/cli.js");
    if !cli.is_file() {
        bail!("web dependencies are missing; run `pnpm install` in web/ first");
    }
    let out_dir = tempfile::tempdir().context("creating a temp dir")?;
    let out = out_dir.path().join("schema.d.ts");
    let status = std::process::Command::new("node")
        .arg(&cli)
        .arg(spec)
        .arg("--output")
        .arg(&out)
        .status()
        .context("running node (is Node.js installed and on PATH?)")?;
    if !status.success() {
        bail!("openapi-typescript failed with {status}");
    }
    let text = std::fs::read_to_string(&out).context("reading the generated types")?;
    Ok(text.replace("\r\n", "\n"))
}

fn app_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../app")
}

fn dart_api_dir() -> PathBuf {
    app_dir().join("lib/api")
}

/// Runs `dart` in `dir`. On Windows `dart` is a `.bat`, which only `cmd` resolves.
fn dart(dir: &Path, args: &[&str]) -> anyhow::Result<()> {
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "dart"]);
        c
    } else {
        std::process::Command::new("dart")
    };
    let status = command
        .args(args)
        .current_dir(dir)
        .status()
        .context("running dart (is the Flutter SDK on PATH?)")?;
    if !status.success() {
        bail!("`dart {}` failed with {status}", args.join(" "));
    }
    Ok(())
}

/// Generates `lib/api` in the Dart package at `dir` (the app, or a check workspace) from the
/// spec its `swagger_parser.yaml` names.
fn generate_dart(dir: &Path) -> anyhow::Result<()> {
    if !app_dir().join(".dart_tool/package_config.json").is_file() {
        bail!("app dependencies are missing; run `flutter pub get` in app/ first");
    }
    dart(dir, &["run", "swagger_parser"])?;
    dart(dir, &["run", "build_runner", "build"])?;
    Ok(())
}

/// Lays out a throwaway copy of the app at `work` with just what the generators read: the
/// pubspec and lock, the resolved package config, and a `swagger_parser.yaml` pointing at
/// `spec`. The committed `lib/api` is not copied, so `--check` never writes to the app and an
/// interrupted run leaves nothing behind.
fn check_workspace(app: &Path, spec: &Path, work: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(work.join(".dart_tool"))?;
    for file in [
        "pubspec.yaml",
        "pubspec.lock",
        ".dart_tool/package_config.json",
        ".dart_tool/package_graph.json",
    ] {
        let from = app.join(file);
        if from.is_file() {
            std::fs::copy(&from, work.join(file))
                .with_context(|| format!("copying {}", from.display()))?;
        }
    }
    let config_path = app.join("swagger_parser.yaml");
    let config = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let spec_path = spec.to_string_lossy().replace('\\', "/");
    let rewritten: Vec<String> = config
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("schema_path:") {
                format!("  schema_path: '{spec_path}'")
            } else {
                line.to_string()
            }
        })
        .collect();
    std::fs::write(work.join("swagger_parser.yaml"), rewritten.join("\n"))
        .context("writing the check workspace's swagger_parser.yaml")?;
    Ok(())
}

/// Every file under `dir` by relative path, with LF line endings.
fn snapshot(dir: &Path) -> anyhow::Result<std::collections::BTreeMap<PathBuf, String>> {
    fn walk(
        root: &Path,
        dir: &Path,
        out: &mut std::collections::BTreeMap<PathBuf, String>,
    ) -> anyhow::Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
            let path = entry?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
            } else {
                let text = std::fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.insert(rel, text.replace("\r\n", "\n"));
            }
        }
        Ok(())
    }
    let mut out = std::collections::BTreeMap::new();
    walk(dir, dir, &mut out)?;
    Ok(out)
}

pub fn run(check: bool, skip_dart: bool) -> anyhow::Result<()> {
    let expected = postit_api::openapi::openapi_json_pretty();
    let path = target();
    let schema = schema_path();
    if check {
        let current = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .replace("\r\n", "\n");
        if current != expected {
            bail!("{} is stale; run `cargo xtask openapi`", path.display());
        }
        // Types come from the in-memory spec, not the on-disk one, so a stale JSON cannot
        // hide stale types.
        let dir = tempfile::tempdir().context("creating a temp dir")?;
        let spec = dir.path().join("openapi.json");
        std::fs::write(&spec, &expected).context("writing the temp spec")?;
        let types = generate_types(&spec)?;
        let committed = std::fs::read_to_string(&schema)
            .map(|t| t.replace("\r\n", "\n"))
            .unwrap_or_default();
        if committed != types {
            bail!("web/src/api/schema.d.ts is stale; run `cargo xtask openapi`");
        }
        // The Dart client is generated from the in-memory spec in a throwaway copy of the app,
        // then compared with the committed one; app/lib/api is never written.
        let api_dir = dart_api_dir();
        let work = dir.path().join("app");
        check_workspace(&app_dir(), &spec, &work)?;
        generate_dart(&work)?;
        if snapshot(&work.join("lib/api"))? != snapshot(&api_dir)? {
            bail!("app/lib/api is stale; run `cargo xtask openapi`");
        }
        println!(
            "{}, {} and {} are current",
            path.display(),
            schema.display(),
            api_dir.display()
        );
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, expected).with_context(|| format!("writing {}", path.display()))?;
    println!("wrote {}", path.display());
    let types = generate_types(&path)?;
    if let Some(dir) = schema.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&schema, types).with_context(|| format!("writing {}", schema.display()))?;
    println!("wrote {}", schema.display());
    if skip_dart {
        // For machines without Flutter. The contract CI job runs --check, which never skips,
        // so a stale Dart client still fails CI.
        eprintln!(
            "warning: skipped {}; run `cargo xtask openapi` with Flutter before committing",
            dart_api_dir().display()
        );
    } else {
        generate_dart(&app_dir())?;
        println!("wrote {}", dart_api_dir().display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_reads_every_file_with_lf_endings() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("{e}"));
        let root = dir.path().join("api");
        std::fs::create_dir_all(root.join("clients")).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(root.join("a.dart"), "one\r\n").unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(root.join("clients/b.dart"), "two\n")
            .unwrap_or_else(|e| unreachable!("{e}"));
        let files = snapshot(&root).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            files.get(Path::new("a.dart")).map(String::as_str),
            Some("one\n")
        );
        assert_eq!(
            files.get(Path::new("clients/b.dart")).map(String::as_str),
            Some("two\n")
        );
    }

    #[test]
    fn check_workspace_copies_only_what_the_generators_read() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("{e}"));
        let app = dir.path().join("app");
        std::fs::create_dir_all(app.join(".dart_tool")).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::create_dir_all(app.join("lib/api")).unwrap_or_else(|e| unreachable!("{e}"));
        for file in [
            "pubspec.yaml",
            "pubspec.lock",
            ".dart_tool/package_config.json",
            ".dart_tool/package_graph.json",
            "lib/api/export.dart",
        ] {
            std::fs::write(app.join(file), file).unwrap_or_else(|e| unreachable!("{e}"));
        }
        std::fs::write(
            app.join("swagger_parser.yaml"),
            "swagger_parser:\n  schema_path: ../api/openapi.json\n  output_directory: lib/api\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        let spec = dir.path().join("spec/openapi.json");
        let work = dir.path().join("work");
        check_workspace(&app, &spec, &work).unwrap_or_else(|e| unreachable!("{e}"));

        for file in [
            "pubspec.yaml",
            "pubspec.lock",
            ".dart_tool/package_config.json",
            ".dart_tool/package_graph.json",
        ] {
            assert!(work.join(file).is_file(), "{file} missing");
        }
        // The committed client is never copied, so the check can only compare against it.
        assert!(!work.join("lib/api").exists());
        let config = std::fs::read_to_string(work.join("swagger_parser.yaml"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let spec_path = spec.to_string_lossy().replace('\\', "/");
        assert!(
            config.contains(&format!("schema_path: '{spec_path}'")),
            "{config}"
        );
        assert!(config.contains("output_directory: lib/api"), "{config}");
    }
}
