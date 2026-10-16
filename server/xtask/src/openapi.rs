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

/// Runs `dart` in `app/`. On Windows `dart` is a `.bat`, which only `cmd` resolves.
fn dart(args: &[&str]) -> anyhow::Result<()> {
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "dart"]);
        c
    } else {
        std::process::Command::new("dart")
    };
    let status = command
        .args(args)
        .current_dir(app_dir())
        .status()
        .context("running dart (is the Flutter SDK on PATH?)")?;
    if !status.success() {
        bail!("`dart {}` failed with {status}", args.join(" "));
    }
    Ok(())
}

/// Regenerates `app/lib/api` from the on-disk `api/openapi.json`.
fn generate_dart() -> anyhow::Result<()> {
    if !app_dir().join(".dart_tool/package_config.json").is_file() {
        bail!("app dependencies are missing; run `flutter pub get` in app/ first");
    }
    dart(&["run", "swagger_parser"])?;
    dart(&["run", "build_runner", "build"])?;
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

/// Puts `dir` back exactly as `files` describes it.
fn restore(dir: &Path, files: &std::collections::BTreeMap<PathBuf, String>) -> anyhow::Result<()> {
    if dir.is_dir() {
        std::fs::remove_dir_all(dir).with_context(|| format!("removing {}", dir.display()))?;
    }
    for (rel, text) in files {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
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
        // The on-disk spec equals the in-memory one here, so the Dart client is generated
        // from it in place and the original files are put back afterwards.
        let api_dir = dart_api_dir();
        let before = snapshot(&api_dir)?;
        let generated = generate_dart().and_then(|()| snapshot(&api_dir));
        restore(&api_dir, &before)?;
        if generated? != before {
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
        generate_dart()?;
        println!("wrote {}", dart_api_dir().display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_puts_a_snapshot_back() {
        let dir = tempfile::tempdir().unwrap_or_else(|e| unreachable!("{e}"));
        let root = dir.path().join("api");
        std::fs::create_dir_all(root.join("clients")).unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(root.join("a.dart"), "one\r\n").unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(root.join("clients/b.dart"), "two\n")
            .unwrap_or_else(|e| unreachable!("{e}"));
        let before = snapshot(&root).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            before.get(Path::new("a.dart")).map(String::as_str),
            Some("one\n")
        );

        std::fs::write(root.join("a.dart"), "changed").unwrap_or_else(|e| unreachable!("{e}"));
        std::fs::write(root.join("new.dart"), "extra").unwrap_or_else(|e| unreachable!("{e}"));
        restore(&root, &before).unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(
            snapshot(&root).unwrap_or_else(|e| unreachable!("{e}")),
            before
        );
    }
}
