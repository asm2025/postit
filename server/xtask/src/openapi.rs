use std::path::PathBuf;

use anyhow::{Context, bail};

fn target() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../api/openapi.json")
}

pub fn run(check: bool) -> anyhow::Result<()> {
    let expected = postit_api::openapi::openapi_json_pretty();
    let path = target();
    if check {
        let current = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .replace("\r\n", "\n");
        if current != expected {
            bail!("{} is stale; run `cargo xtask openapi`", path.display());
        }
        println!("{} is current", path.display());
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, expected).with_context(|| format!("writing {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}
