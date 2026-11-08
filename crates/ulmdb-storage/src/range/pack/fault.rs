//! Hooks are compiled out of ordinary builds.
use crate::range::pack::Result;
#[inline]
pub(crate) fn point(name: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    {
        use std::io::Write;
        if let Some(path) = std::env::var_os("ULMDB_PACKED_TRACE_FILE") {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            writeln!(file, "{name}")?;
        }
        if std::env::var("ULMDB_PACKED_CRASH_AT").is_ok_and(|value| value == name) {
            std::process::exit(86);
        }
        if std::env::var("ULMDB_PACKED_FAIL_AT").is_ok_and(|value| value == name) {
            return Err(crate::range::pack::Error::Io(std::io::Error::other(
                "injected packed IO error",
            )));
        }
    }
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}
