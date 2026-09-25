fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);

    match args.next().as_deref() {
        Some("--version") => {
            println!("postly {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some(other) => anyhow::bail!("unknown argument: {other}"),
        None => anyhow::bail!("usage: postly --version"),
    }
}
