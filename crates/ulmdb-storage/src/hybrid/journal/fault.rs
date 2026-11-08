use crate::Result;
pub(crate) fn point(name: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    {
        use std::io::Write;
        if let Some(p) = std::env::var_os("ULMDB_HYBRID_TRACE") {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)?;
            writeln!(f, "{name}")?;
        }
        if std::env::var("ULMDB_HYBRID_EXIT_AT").is_ok_and(|s| s == name) {
            std::process::exit(86);
        }
        if std::env::var("ULMDB_HYBRID_FAIL_AT").is_ok_and(|s| s == name) {
            return Err(std::io::Error::other("injected hybrid IO failure").into());
        }
    }
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}
