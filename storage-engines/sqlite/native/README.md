# Private SQLite native bridge

Builds the optional `ulmdb_owned` SQLite backend; see the [SQL guide](../README.md) for use and limits.

Run from the repository root with Linux and Rust 1.94:

```sh
cargo build --release --locked --features versioned \
  --manifest-path storage-engines/sqlite/native/Cargo.toml \
  --target-dir target/sqlite-native-owned-versioned

cargo test --locked --features versioned \
  --manifest-path storage-engines/sqlite/native/Cargo.toml
```

Library: `target/sqlite-native-owned-versioned/release/libulmdb_sqlite_native.a`.

```sh
make -C storage-engines/sqlite owned
make -C storage-engines/sqlite test-owned
make -C storage-engines/sqlite test-owned-capacity
```

The owned Make target selects `versioned`; standalone Cargo defaults remain
empty and ULMDB core defaults are unchanged. Set `ULMDB_NATIVE_LIB` to an
absolute prebuilt library path to skip Cargo during extension linking.

[Private frontend ABI](owned_backend.h); not a public LMDB interface.
