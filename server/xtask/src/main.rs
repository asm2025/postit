mod openapi;
mod zitadel_bootstrap;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("zitadel-bootstrap") => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(zitadel_bootstrap::run())
        }
        Some("openapi") => {
            let flags: Vec<String> = args.collect();
            let check = flags.iter().any(|f| f == "--check");
            let skip_dart = flags.iter().any(|f| f == "--skip-dart");
            if flags.iter().any(|f| f != "--check" && f != "--skip-dart") || (check && skip_dart) {
                anyhow::bail!("usage: xtask openapi [--check | --skip-dart]");
            }
            openapi::run(check, skip_dart)
        }
        Some(other) => anyhow::bail!("unknown xtask: {other}"),
        None => anyhow::bail!("usage: xtask <zitadel-bootstrap|openapi [--check | --skip-dart]>"),
    }
}
