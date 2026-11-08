//! Record current checked-out runtime and benchmark inputs, not archived sources.
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};
fn add(root: &Path, p: &Path, out: &mut BTreeMap<String, String>) {
    println!("cargo:rerun-if-changed={}", p.display());
    out.insert(
        p.strip_prefix(root).unwrap().to_string_lossy().into_owned(),
        format!("{:x}", Sha256::digest(fs::read(p).unwrap())),
    );
}
fn tree(root: &Path, p: &Path, out: &mut BTreeMap<String, String>) {
    println!("cargo:rerun-if-changed={}", p.display());
    for entry in fs::read_dir(p).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            tree(root, &p, out)
        } else {
            add(root, &p, out)
        }
    }
}
fn main() {
    let package = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = package.join("../..").canonicalize().unwrap();
    let mut files = BTreeMap::new();
    for name in [
        "ulmdb",
        "ulmdb-core",
        "ulmdb-kv",
        "ulmdb-prefix",
        "ulmdb-storage",
    ] {
        let p = root.join("crates").join(name);
        tree(&root, &p.join("src"), &mut files);
        add(&root, &p.join("Cargo.toml"), &mut files);
    }
    for relative in ["benchmark/engine", "benchmark/support/lmdb"] {
        let p = root.join(relative);
        tree(&root, &p.join("src"), &mut files);
        for name in ["Cargo.toml", "Cargo.lock", "build.rs"] {
            if p.join(name).is_file() {
                add(&root, &p.join(name), &mut files);
            }
        }
    }
    let support = root.join("benchmark/support/lmdb");
    tree(&root, &support.join("vendor"), &mut files);
    add(&root, &support.join("source.toml"), &mut files);
    for name in ["Cargo.toml", "Cargo.lock"] {
        if root.join(name).is_file() {
            add(&root, &root.join(name), &mut files);
        }
    }
    let mut fields: BTreeMap<String, String> = env::vars()
        .filter(|(k, _)| {
            k.starts_with("CARGO_FEATURE_")
                || k.starts_with("CARGO_PROFILE_")
                || matches!(
                    k.as_str(),
                    "PROFILE"
                        | "OPT_LEVEL"
                        | "DEBUG"
                        | "HOST"
                        | "TARGET"
                        | "CARGO_CFG_TARGET_FEATURE"
                        | "RUSTFLAGS"
                        | "CARGO_ENCODED_RUSTFLAGS"
                        | "RUSTC"
                        | "RUSTC_WRAPPER"
                        | "RUSTC_WORKSPACE_WRAPPER"
                )
        })
        .collect();
    let info = Command::new(env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()))
        .arg("-vV")
        .output()
        .unwrap();
    assert!(info.status.success());
    fields.insert(
        "compiler_identity".into(),
        String::from_utf8(info.stdout).unwrap(),
    );
    let mut output = String::new();
    for (name, values) in [("SOURCE_HASHES", files), ("BUILD_FIELDS", fields)] {
        output.push_str(&format!("pub const {name}: &[(&str,&str)] = &[\n"));
        for (k, v) in values {
            output.push_str(&format!("({k:?},{v:?}),\n"));
        }
        output.push_str("];\n");
    }
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("provenance.rs"),
        output,
    )
    .unwrap();
}
