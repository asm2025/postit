fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version") => {
            println!("postit {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("healthcheck") => postit_server::healthcheck_from_env(),
        Some(other) => {
            anyhow::bail!("unknown argument: {other} (usage: postit [--version | healthcheck])")
        }
        None => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(postit_server::run_from_env()),
    }
}
