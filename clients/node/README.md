# ulmdb-js

A synchronous Node.js client for the ULMDB engine. It provides ordered binary
key/value databases, sorted duplicate values, and native IPv4/IPv6 prefix indexes.
The client has no npm runtime dependencies and uses a small Node N-API addon.

From the repository root, build the shared library and the addon:

```sh
cargo build -p ulmdb-c --locked
cd clients/node
npm run build
npm test
npm run example
```

Node.js 18 or newer, a C compiler, and Node development headers are required.
If headers are not beside your Node installation or in `/usr/include/node`, set
`NODE_INCLUDE_DIR` to the directory containing `node_api.h`. The build currently
supports Linux and macOS. No Windows addon build is supplied.

The addon loads `target/debug/libulmdb_c.so` (`.dylib` on macOS) by default.
Set `ULMDB_LIBRARY` to an absolute shared-library path to select a release build
or use the client outside this source checkout. The runtime override is checked
when the module is first loaded. To package the client separately, ship or build
the ULMDB shared library and set that variable; the npm package does not embed it.

```js
const { Environment } = require('ulmdb-js');

const env = new Environment('./data');
try {
  env.write(tx => {
    const hosts = tx.openDatabase('hosts', { create: true });
    hosts.put('example.test', Buffer.from('192.0.2.130'));

    const routes = tx.openDatabase('routes4', { kind: 'ipv4', create: true });
    routes.putPrefix('192.0.2.0/24', 'example network');
  });
  env.read(tx => {
    console.log(tx.openDatabase('hosts').get('example.test').toString());
    const routes = tx.openDatabase('routes4', { kind: 'ipv4' });
    console.log(routes.longestMatch('192.0.2.130'));
    // { prefix: '192.0.2.0/24', value: <Buffer ...> }
  });
} finally {
  env.close();
}
```

Use `kind: 'ipv6'` for IPv6 CIDRs, or `kind: 'duplicates'` for sorted unique
values per key. Prefix databases provide `putPrefix`, `getPrefix`,
`deletePrefix`, and `longestMatch`. Missing reads return `null`; deletes return
whether a record existed. Database failures throw an `Error` with a numeric
LMDB-compatible `code`. `noOverwrite` and `noDuplicateData` options on `put`
report existing records with code `-30799`.

Keys and values accept UTF-8 strings, `Buffer`, or `Uint8Array`. Reads return
copied `Buffer` instances that remain valid after the transaction ends. Empty
values are distinct from missing values. `entries({ reverse: true })` returns
copied pairs in descending byte order; it eagerly materializes the complete
database, including every duplicate pair.

`env.write(callback)` commits when the callback returns and aborts if it throws.
`env.read(callback)` pins one snapshot until the callback returns. Independent
read transactions can overlap; the client selects `MDB_NOTLS` so each owns its
reader slot. Callbacks must
be synchronous; Promise results are rejected. For explicit lifetimes use
`env.begin({ readOnly: false })` and finish with `commit()` or `abort()` in a
`try`/`finally` block. Database objects belong to their transaction and cannot be
reused afterward. Only one writer may be active in each environment. Finish all
transactions before calling `close()`. Operations run synchronously and block
the calling Node thread; use a worker if the application needs event-loop isolation.

This client uses ULMDB's format and the C compatibility layer. Existing LMDB
data files cannot be opened directly; the C client's compatibility matrix lists
the supported LMDB behaviors and remaining differences.

See [the runnable example](examples/basic.js) and [integration tests](test/client.test.js).
