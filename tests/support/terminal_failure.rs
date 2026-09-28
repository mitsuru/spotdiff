fn main() -> anyhow::Result<()> {
    let _session = spotdiff::terminal::Session::enter()?;
    if std::env::args().nth(1).as_deref() == Some("panic") {
        panic!("test panic");
    }
    anyhow::bail!("test error")
}
