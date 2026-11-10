fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_BTREE").is_some() {
        // These private modules are outside this crate's package directory.
        println!("cargo:rerun-if-changed=../btree");
    }
}
