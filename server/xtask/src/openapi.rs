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

pub fn run(check: bool) -> anyhow::Result<()> {
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
        println!("{} and {} are current", path.display(), schema.display());
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
    Ok(())
}
