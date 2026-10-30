mod openapi;
mod vapid;
mod zitadel_bootstrap;
mod zitadel_messages;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("zitadel-bootstrap") => {
            let flags: Vec<String> = args.collect();
            if flags.iter().any(|f| f != "--check") {
                anyhow::bail!("usage: xtask zitadel-bootstrap [--check]");
            }
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(zitadel_bootstrap::run(!flags.is_empty()))
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
        Some("vapid-keygen") => {
            if args.next().is_some() {
                anyhow::bail!("usage: xtask vapid-keygen");
            }
            vapid::run()
        }
        Some(other) => anyhow::bail!("unknown xtask: {other}"),
        None => anyhow::bail!(
            "usage: xtask <zitadel-bootstrap [--check]|openapi [--check | --skip-dart]|vapid-keygen>"
        ),
    }
}
