# ulmdb-js

Synchronous Node.js 18+ client. Requires Cargo, a C compiler and Node headers; Linux/macOS.

## Build

```sh
# From the repository root:
cargo build -p ulmdb-c --locked
npm --prefix clients/node run build
```

- Missing `node_api.h`: set `NODE_INCLUDE_DIR` to its directory.
- Outside the checkout: set `ULMDB_LIBRARY` to the shared library's absolute path before loading the module.

## Use

```js
// Save as app.js in the repository root; run with node app.js.
const { Environment } = require('./clients/node');

const env = new Environment('./example-db');
try {
  env.write(tx => {
    tx.openDatabase('people', { create: true }).put('alice', 'Alice');
  });
  env.read(tx => {
    console.log(tx.openDatabase('people').get('alice').toString());
  });
} finally {
  env.close();
}
```

- Callbacks must be synchronous; writes commit on return and abort on throw.
- Reopen database objects in each transaction.
- Keys/values accept strings, `Buffer` or `Uint8Array`; reads return `Buffer` or `null`.
- [IPv4/IPv6 example](examples/basic.js) · [TypeScript API](index.d.ts) · [C API and limits](../c/README.md)

## Test

```sh
npm --prefix clients/node test
npm --prefix clients/node run example
```
