# LMDB adapter for Windows

## Build

```sh
# Repository root; requires Python 3, Rust Windows GNU target and MinGW
rustup target add x86_64-pc-windows-gnu
cargo build --locked --release -p ulmdb-c --target x86_64-pc-windows-gnu
python3 clients/lmdb-ulmdb/build_windows_tools.py \
  --modern-source benchmark/support/lmdb/vendor \
  --legacy-source "$HOME/.cache/ulmdb-compat/lmdb-0.9.31" \
  --dll target/x86_64-pc-windows-gnu/release/ulmdb_c.dll \
  --import-library target/x86_64-pc-windows-gnu/release/libulmdb_c.dll.a \
  --output target/lmdb-ulmdb-windows
```

- Output directory must be absent.
- Legacy sources must match [these pins](legacy-source.json); the builder does not fetch them.
- Default tools: `x86_64-w64-mingw32-gcc`, `x86_64-w64-mingw32-objdump`.
- Override tool paths with `--cc` and `--objdump`.

## Use

```text
target/lmdb-ulmdb-windows/bin/convert-lmdb-to-ulmdb.exe --abi 1.0.2 C:/apps/application.exe
target/lmdb-ulmdb-windows/bin/ulmdb-migrate.exe C:/data/lmdb C:/data/new-ulmdb
```

- A competing LMDB DLL beside the application must be replaced explicitly.
- Static LMDB, private DLL names and absolute loader bindings require application build changes.
- [Migration and ABI options](README.md#use)

## Test

- Cross-compilation checks builds and linking; Windows runtime compatibility remains unqualified.
- [Platform limits and evidence](PARITY_GAPS.md)
