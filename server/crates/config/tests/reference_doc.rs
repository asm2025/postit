//! Every setting is documented in `docs/configuration.md` under its environment
//! variable name, so the reference cannot fall behind the settings tree.

mod common;

use postit_config::{Environment, load_with};

use common::{production_vars, shipped_config_dir};

fn env_names(prefix: &str, value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                env_names(&format!("{prefix}__{}", key.to_uppercase()), child, out);
            }
        }
        _ => out.push(prefix.to_owned()),
    }
}

#[test]
fn every_setting_is_in_the_reference() {
    let settings = load_with(
        Environment::Production,
        &shipped_config_dir(),
        production_vars(),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let dump = settings
        .redacted_dump()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut names = Vec::new();
    env_names("POSTIT", &dump, &mut names);

    let doc_path = shipped_config_dir().join("../../docs/configuration.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| unreachable!("reading {}: {e}", doc_path.display()));
    // A name counts only as the first cell of a reference-table row, not as a mention in
    // prose or in another table.
    let missing: Vec<&String> = names
        .iter()
        .filter(|name| {
            let row = format!("| `{name}` |");
            !doc.lines().any(|line| line.starts_with(&row))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "docs/configuration.md is missing {} setting(s):\n{}",
        missing.len(),
        missing
            .iter()
            .map(|n| format!("  {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
