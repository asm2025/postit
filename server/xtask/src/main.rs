mod openapi;
mod zitadel_bootstrap;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("zitadel-bootstrap") => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(zitadel_bootstrap::run())
        }
        Some("openapi") => openapi::run(args.next().as_deref() == Some("--check")),
        Some(other) => anyhow::bail!("unknown xtask: {other}"),
        None => anyhow::bail!("usage: xtask <zitadel-bootstrap|openapi [--check]>"),
    }
}
