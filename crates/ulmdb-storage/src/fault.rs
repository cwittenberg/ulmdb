//! Explicit opt-in subprocess crash points. This module is absent from normal
//! builds; the environment variable has no effect without `fault-injection`.
pub(crate) fn point(name: &str) -> crate::Result<()> {
    if let Some(path) = std::env::var_os("ULMDB_TRACE_FILE") {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        writeln!(file, "{name}")?;
    }
    // Explicit fault-build-only pipe rendezvous for real publication/capture
    // schedules. EOF releases ownership through the normal IO-error path.
    if std::env::var("ULMDB_PAUSE_AT").is_ok_and(|requested| requested == name) {
        use std::io::{Read, Write};
        let mut out = std::io::stdout().lock();
        writeln!(out, "PAUSED point={name}")?;
        out.flush()?;
        drop(out);
        let mut release = [0u8; 1];
        std::io::stdin().read_exact(&mut release)?;
    }
    if std::env::var("ULMDB_CRASH_AT").is_ok_and(|requested| requested == name) {
        // Exit without Rust destructors. This tests process interruption, not
        // loss of the kernel's dirty cache or storage-controller power loss.
        std::process::exit(86);
    }
    if std::env::var("ULMDB_FAIL_AT").is_ok_and(|requested| requested == name) {
        return Err(crate::Error::Io(std::io::Error::other(format!(
            "injected storage I/O failure at {name}"
        ))));
    }
    Ok(())
}

/// Gate rendezvous is opt-in, preserving existing ordinary publication traces.
pub(crate) fn history_point(name: &str) -> crate::Result<()> {
    if ["ULMDB_PAUSE_AT", "ULMDB_FAIL_AT", "ULMDB_CRASH_AT"]
        .iter()
        .any(|key| std::env::var(key).is_ok_and(|value| value == name))
    {
        point(name)?;
    }
    Ok(())
}
