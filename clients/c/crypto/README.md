# OpenSSL AES-256-GCM module

Optional `MDB_crypto` callback module; separate from the engine build.

## Build

- Linux, GNU-compatible linker, C compiler, Make, `pkg-config`, OpenSSL `libcrypto >= 3.0`.
- Runtime: compatible system `libcrypto` and the ULMDB C provider.

```sh
# Repository root
cargo build -p ulmdb-c --locked
make -C clients/c/crypto
```

Output: `target/crypto/libulmdb_aes256gcm.so`.
Make overrides: `CC`, `PKG_CONFIG`, `CFLAGS`, `OUTPUT`.

## Use

| Call | Purpose |
| --- | --- |
| `mdb_modload(module_path, 0, &crypto, &error)` | Load module; stop if it returns `NULL` |
| `mdb_env_create(&env)` | Create environment; check return code |
| `mdb_modsetup(env, crypto, secret_hex)` | Configure callbacks and key before opening |
| `mdb_env_open(env, path, flags, mode)` | Open encrypted environment; check return code |
| `mdb_modunload(module)` | Unload after all environments and retained reads close |

- Key: exactly 64 hexadecimal characters = 32 secret bytes; no passphrase derivation.
- Supply your own secret. Keys in [the full C example](test_engine.c) are public test material.
- Keep the module loaded while environments, transactions or retained reads can invoke callbacks.
- Engine/direct caller: valid buffers and a unique IV for every encryption under a key.
- Native engine: 16-byte IV and 16-byte authentication tag; no associated-data argument.
- Authentication failure: nonzero result; destination buffer cleared. Check every operation's return code.

## Test

```sh
python3 clients/c/crypto/test_vectors.py target/crypto/libulmdb_aes256gcm.so

cc -I clients/c/include clients/c/crypto/test_engine.c \
  -L target/debug -Wl,-rpath,"$PWD/target/debug" -lulmdb_c \
  -o target/crypto/test-engine
ulmdb_crypto_workdir=$(mktemp -d)
target/crypto/test-engine "$PWD/target/crypto/libulmdb_aes256gcm.so" "$ulmdb_crypto_workdir"
```

- [Vector tests](test_vectors.py): pinned AES-GCM vectors, tampering, key parsing, buffers and parallel contexts.
- [Engine test](test_engine.c): loader, encrypted writes, copies, incremental load and reopen.
- `--expect-load-einval`: require an older provider's known incremental-load rejection; does not count as a passing load.
