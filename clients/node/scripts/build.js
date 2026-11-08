'use strict';

const { spawnSync } = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');

const client = path.resolve(__dirname, '..');
const root = path.resolve(client, '../..');
const extension = process.platform === 'darwin' ? 'dylib' : 'so';
if (!['linux', 'darwin'].includes(process.platform)) {
  throw new Error('The native build currently supports Linux and macOS.');
}
const candidates = [
  process.env.NODE_INCLUDE_DIR,
  path.resolve(path.dirname(process.execPath), '../include/node'),
  '/usr/include/node',
  '/usr/local/include/node',
].filter(Boolean);
const headers = candidates.find(candidate => fs.existsSync(path.join(candidate, 'node_api.h')));
if (!headers) throw new Error('Node headers missing: set NODE_INCLUDE_DIR to the directory containing node_api.h.');
const library = path.resolve(process.env.ULMDB_LIBRARY || path.join(root, `target/debug/libulmdb_c.${extension}`));
fs.mkdirSync(path.join(client, 'build'), { recursive: true });
const args = [
  '-std=c11', '-O2', '-Wall', '-Wextra', '-Werror', '-fPIC', '-shared', '-pthread',
  `-I${headers}`, `-DULMDB_DEFAULT_LIBRARY=${JSON.stringify(library)}`,
  path.join(client, 'native.c'), '-o', path.join(client, 'build/ulmdb.node'),
];
if (process.platform === 'darwin') args.push('-undefined', 'dynamic_lookup');
else args.push('-ldl');
const result = spawnSync(process.env.CC || 'cc', args, { stdio: 'inherit' });
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status || 1);
console.log(`Built ${path.join(client, 'build/ulmdb.node')}`);
