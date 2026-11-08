use std::{env, fs, path::PathBuf, process::Command};
fn main() {
    println!("cargo:rerun-if-env-changed=SQLITE3_LIB_DIR");
    let target = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let mut dirs = Vec::new();
    if let Some(dir) = env::var_os("SQLITE3_LIB_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    dirs.extend(
        [
            format!("/usr/lib/{target}-linux-gnu"),
            format!("/lib/{target}-linux-gnu"),
            "/usr/lib64".into(),
            "/usr/lib".into(),
        ]
        .map(PathBuf::from),
    );
    let library = dirs.iter().map(|d| d.join("libsqlite3.so")).find(|p| p.exists())
        .or_else(|| dirs.iter().map(|d| d.join("libsqlite3.so.0")).find(|p| p.exists()))
        .expect("Install the system SQLite development library (Debian/Ubuntu: libsqlite3-dev), or set SQLITE3_LIB_DIR");
    let library = fs::canonicalize(library).unwrap();
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // Linux runtime-only installations may omit the unversioned linker name.
    // This symlink lives exclusively in this benchmark's build directory.
    #[cfg(unix)]
    {
        let link = out.join("libsqlite3.so");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&library, &link).unwrap();
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=dylib=sqlite3");
    println!("cargo:rerun-if-changed={}", library.display());
    println!(
        "cargo:rustc-env=BENCH_SQLITE_LINK_PATH={}",
        library.display()
    );
    let rustc = Command::new(env::var_os("RUSTC").unwrap())
        .arg("-Vv")
        .output()
        .unwrap();
    fs::write(out.join("rustc.txt"), rustc.stdout).unwrap();
    println!(
        "cargo:rustc-env=BENCH_RUSTC_INFO={}",
        out.join("rustc.txt").display()
    );
}
