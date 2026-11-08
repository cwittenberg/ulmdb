use crate::Result;
pub(super) fn point(name: &str) -> Result<()> {
    #[cfg(feature = "fault-injection")]
    {
        use std::io::Write;
        if let Some(p) = std::env::var_os("ULMDB_H5_TRACE") {
            writeln!(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)?,
                "{name}"
            )?;
        }
        if std::env::var("ULMDB_H5_EXIT_AT").is_ok_and(|s| s == name) {
            std::process::exit(86);
        }
        if std::env::var("ULMDB_H5_FAIL_AT").is_ok_and(|s| s == name) {
            return Err(std::io::Error::other("injected H5 artifact IO failure").into());
        }
    }
    #[cfg(not(feature = "fault-injection"))]
    let _ = name;
    Ok(())
}
