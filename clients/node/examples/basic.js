'use strict';

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { Environment } = require('..');

const temporary = !process.argv[2];
const parent = temporary ? fs.mkdtempSync(path.join(os.tmpdir(), 'ulmdb-js-example-')) : null;
const environment = new Environment(process.argv[2] || path.join(parent, 'database'));
try {
  environment.write(tx => {
    tx.openDatabase('hosts', { create: true }).put('example.test', '192.0.2.130');
    tx.openDatabase('routes4', { create: true, kind: 'ipv4' }).putPrefix('192.0.2.0/24', 'example network');
    tx.openDatabase('routes6', { create: true, kind: 'ipv6' }).putPrefix('2001:db8::/32', 'IPv6 example');
  });
  environment.read(tx => {
    console.log('Host:', tx.openDatabase('hosts').get('example.test').toString());
    for (const [database, kind, address] of [
      ['routes4', 'ipv4', '192.0.2.130'],
      ['routes6', 'ipv6', '2001:db8::1234'],
    ]) {
      const match = tx.openDatabase(database, { kind }).longestMatch(address);
      console.log(`${address}: ${match.prefix} -> ${match.value.toString()}`);
    }
  });
} finally {
  environment.close();
  if (temporary) fs.rmSync(parent, { recursive: true, force: true });
}
