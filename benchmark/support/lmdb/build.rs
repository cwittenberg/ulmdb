//! Compile only the pinned external comparison engine; never a production backend.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

const COMMIT: &str = "c279e0477c713076893e8c2b1a6337e421c15867";
const FLAGS: &[&str] = &["-O3", "-g", "-fPIC", "-pthread"];

#[derive(Deserialize)]
struct Pins {
    schema: u32,
    sources: Vec<Source>,
}
#[derive(Deserialize)]
struct Source {
    id: String,
    commit: String,
    files: Vec<File>,
}
#[derive(Deserialize)]
struct File {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct BuildInfo {
    source_commit: &'static str,
    source_pin_manifest_sha256: String,
    compiler: String,
    compiler_version: String,
    compiler_target: String,
    rustc: String,
    rust_profile: String,
    rust_opt_level: String,
    rust_debug: String,
    rust_target_features: String,
    rust_environment: BTreeMap<String, String>,
    adapter_manifest_sha256: String,
    adapter_lock_sha256: String,
    c_flags: Vec<&'static str>,
    archive_sha256: String,
    link_libraries: Vec<&'static str>,
}

fn digest(path: &Path) -> String {
    format!(
        "{:x}",
        Sha256::digest(fs::read(path).expect("read pinned input"))
    )
}
fn run(command: &mut Command) -> String {
    let shown = format!("{command:?}");
    let output = command.output().unwrap_or_else(|e| panic!("{shown}: {e}"));
    assert!(
        output.status.success(),
        "{shown}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
fn verify(source: &Path, pin: &Source) {
    for name in [
        "mdb.c",
        "midl.c",
        "module.c",
        "lmdb.h",
        "midl.h",
        "LICENSE",
        "COPYRIGHT",
    ] {
        let relative = format!("libraries/liblmdb/{name}");
        let file = pin
            .files
            .iter()
            .find(|f| f.path == relative)
            .unwrap_or_else(|| panic!("pin manifest must include {relative}"));
        let path = source.join(name);
        assert!(
            !fs::symlink_metadata(&path)
                .expect("pinned regular source")
                .file_type()
                .is_symlink(),
            "source symlink rejected"
        );
        assert_eq!(
            fs::metadata(&path).unwrap().len(),
            file.bytes,
            "size mismatch: {name}"
        );
        assert_eq!(digest(&path), file.sha256, "SHA-256 mismatch: {name}");
        println!("cargo:rerun-if-changed={}", path.display());
    }
}
fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let pins_path = manifest.join("source.toml");
    let pins: Pins =
        toml::from_str(&fs::read_to_string(&pins_path).expect("read upstream pin manifest"))
            .unwrap();
    assert_eq!(pins.schema, 1);
    let pin = pins
        .sources
        .iter()
        .find(|s| s.id == "LMDB")
        .expect("LMDB source pin");
    assert_eq!(
        pin.commit, COMMIT,
        "review changed upstream source before rebuilding baseline"
    );
    let source = manifest.join("vendor");
    verify(&source, pin);
    let host = env::var("HOST").unwrap();
    let target = env::var("TARGET").unwrap();
    assert_eq!(host, target, "this adapter builds native comparisons only");
    assert!(
        target.ends_with("-linux-gnu")
            && (target.starts_with("x86_64-") || target.starts_with("aarch64-")),
        "unsupported comparison target {target}"
    );
    let cc = env::var_os("CC").unwrap_or_else(|| "cc".into());
    let ar = env::var_os("AR").unwrap_or_else(|| "ar".into());
    let version = run(Command::new(&cc).arg("--version"));
    let machine = run(Command::new(&cc).arg("-dumpmachine"));
    let rustc =
        run(Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into())).arg("-vV"));
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut objects = Vec::new();
    for name in ["mdb", "midl", "module"] {
        let object = out.join(format!("{name}.o"));
        run(Command::new(&cc)
            .args(FLAGS)
            .arg("-c")
            .arg(source.join(format!("{name}.c")))
            .arg("-o")
            .arg(&object));
        objects.push(object);
    }
    let library = out.join("libulmdb_baseline_lmdb.a");
    run(Command::new(&ar).arg("crs").arg(&library).args(&objects));
    verify(&source, pin);
    let rust_environment = env::vars()
        .filter(|(key, _)| {
            key.starts_with("CARGO_PROFILE_")
                || key.starts_with("CARGO_TARGET_") && key.ends_with("RUSTFLAGS")
                || matches!(
                    key.as_str(),
                    "RUSTFLAGS"
                        | "CARGO_ENCODED_RUSTFLAGS"
                        | "RUSTC"
                        | "RUSTC_WRAPPER"
                        | "RUSTC_WORKSPACE_WRAPPER"
                        | "RUSTUP_TOOLCHAIN"
                )
        })
        .collect::<BTreeMap<_, _>>();
    for key in rust_environment.keys() {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let info = toml::to_string_pretty(&BuildInfo {
        source_commit: COMMIT,
        source_pin_manifest_sha256: digest(&pins_path),
        compiler: cc.to_string_lossy().into_owned(),
        compiler_version: version,
        compiler_target: machine,
        rustc,
        rust_profile: env::var("PROFILE").unwrap(),
        rust_opt_level: env::var("OPT_LEVEL").unwrap(),
        rust_debug: env::var("DEBUG").unwrap_or_default(),
        rust_target_features: env::var("CARGO_CFG_TARGET_FEATURE").unwrap_or_default(),
        rust_environment,
        adapter_manifest_sha256: digest(&manifest.join("Cargo.toml")),
        adapter_lock_sha256: digest(&manifest.join("Cargo.lock")),
        c_flags: FLAGS.to_vec(),
        archive_sha256: digest(&library),
        link_libraries: vec!["pthread", "dl"],
    })
    .expect("serialize effective build profile");
    let build_info = out.join("build-info.toml");
    fs::write(&build_info, info).unwrap();
    println!(
        "cargo:rustc-env=ULMDB_LMDB_BUILD_INFO={}",
        build_info.display()
    );
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=ulmdb_baseline_lmdb");
    println!("cargo:rustc-link-lib=pthread");
    println!("cargo:rustc-link-lib=dl");
    println!("cargo:rerun-if-changed={}", pins_path.display());
    for key in [
        "CC",
        "AR",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
}
