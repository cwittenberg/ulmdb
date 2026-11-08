# ULMDB clients

## Build

```sh
# From the repository root
cargo build -p ulmdb-c --locked
```

## Use

| Client | Package / interface |
| --- | --- |
| [C](c/README.md) | `ulmdb-c` / `libulmdb_c`; `mdb_*` API + IP extensions |
| [C++17](cpp/README.md) | RAII headers |
| [Rust](rust/README.md) | `ulmdb-rs`; native API |
| [Python](python/README.md) | `ulmdb`; `ctypes` |
| [PHP](php/README.md) | `ulmdb/ulmdb`; FFI |
| [Node.js](node/README.md) | `ulmdb-js`; N-API + TypeScript |
| [LMDB adapter](lmdb-ulmdb/README.md) | Replacement `liblmdb` + data migration |

- Setup and examples: follow the client links.
- Python/PHP/Node outside this checkout: set `ULMDB_LIBRARY` to the shared library's absolute path.
- [Engine status and benchmarks](../README.md) · [LMDB compatibility](lmdb-ulmdb/PARITY_GAPS.md)

## Test

```sh
# From the repository root
./clients/test.sh
```

- Requires pinned LMDB sources, Node development headers and Docker for PHP.
