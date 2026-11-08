# ULMDB PHP client

PHP 8.1+ · FFI · package `ulmdb/ulmdb` · namespace `Ulmdb`

## Build

```sh
# Repository root; requires Docker.
docker build -f clients/php/Dockerfile -t ulmdb-php-test:local .
```

## Use

```php
<?php
// app.php in the repository root
require __DIR__ . '/clients/php/autoload.php';

use Ulmdb\Environment;
use Ulmdb\Transaction;

$env = new Environment('/tmp/my-database');
try {
    $env->write(function (Transaction $tx): void {
        $tx->openDatabase('people', create: true)->put('alice', 'Alice');
    });
    echo $env->read(fn (Transaction $tx) =>
        $tx->openDatabase('people')->get('alice'));
} finally {
    $env->close();
}
```

```sh
docker run --rm --network none \
  -v "$PWD/app.php:/work/ulmdb/app.php:ro" ulmdb-php-test:local php app.php
```

- `write()` commits on success; exceptions abort. Database handles stay inside their transaction.
- Database kinds: `kv`, `duplicates`, `ipv4`, `ipv6`.
- `get()` returns a binary string or `null`; `entries()` copies all ordered pairs.
- Outside this image: enable `ext-ffi` and `ffi.enable=true`; set `ULMDB_LIBRARY` to the absolute ULMDB shared-library path.
- Build that library with `cargo build -p ulmdb-c --locked`: `target/debug/libulmdb_c.so` on Linux, `libulmdb_c.dylib` on macOS.

[IPv4/IPv6 example](examples/basic.php) · [API source](src/) · [LMDB migration](../lmdb-ulmdb/README.md)

## Test

```sh
./clients/php/test-docker.sh
```
