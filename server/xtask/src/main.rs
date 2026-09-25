fn main() -> anyhow::Result<()> {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some(other) => anyhow::bail!("unknown xtask: {other}"),
        None => anyhow::bail!("usage: xtask <task>"),
    }
}
