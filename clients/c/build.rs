fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_SQL_BATCH").is_some() {
        println!("cargo:rerun-if-changed=../../storage-engines/postgresql/native");
    }
}
