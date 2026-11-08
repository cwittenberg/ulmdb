'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { Environment } = require('..');

function fixture(t) {
  const parent = fs.mkdtempSync(path.join(os.tmpdir(), 'ulmdb-js-'));
  const directory = path.join(parent, 'database');
  const environment = new Environment(directory);
  t.after(() => { environment.close(); fs.rmSync(parent, { recursive: true, force: true }); });
  return { directory, environment };
}

test('commits binary values, ordered entries, deletion, and persistent reopen', t => {
  const { directory, environment } = fixture(t);
  environment.write(tx => {
    const db = tx.openDatabase('items', { create: true });
    db.put(Buffer.from([0, 255]), Buffer.from([4, 0, 254]));
    db.put('z', new Uint8Array([1, 2, 3]));
    db.put('empty', Buffer.alloc(0));
    assert.deepEqual(db.get(Buffer.from([0, 255])), Buffer.from([4, 0, 254]));
    assert.equal(db.get('absent'), null);
    assert.throws(() => db.put('z', 'replacement', { noOverwrite: true }), { code: -30799 });
    assert.equal(db.delete('absent'), false);
  });
  const copied = environment.read(tx => {
    const db = tx.openDatabase('items');
    assert.deepEqual(db.entries().map(([key]) => key), [Buffer.from([0, 255]), Buffer.from('empty'), Buffer.from('z')]);
    assert.deepEqual(db.entries({ reverse: true }).map(([key]) => key), [Buffer.from('z'), Buffer.from('empty'), Buffer.from([0, 255])]);
    assert.deepEqual(db.get('empty'), Buffer.alloc(0));
    const independent = db.get('z');
    independent.fill(255);
    assert.deepEqual(db.get('z'), Buffer.from([1, 2, 3]));
    return db.get('z');
  });
  assert.deepEqual(copied, Buffer.from([1, 2, 3]));
  environment.close();
  assert.deepEqual(copied, Buffer.from([1, 2, 3]));
  const reopened = new Environment(directory);
  try {
    reopened.write(tx => assert.equal(tx.openDatabase('items').delete('z'), true));
    assert.equal(reopened.read(tx => tx.openDatabase('items').get('z')), null);
    assert.deepEqual(reopened.read(tx => tx.openDatabase('items').get(Buffer.from([0, 255]))), Buffer.from([4, 0, 254]));
  } finally { reopened.close(); }
});

test('rollback and pinned snapshots preserve transaction boundaries', t => {
  const { environment } = fixture(t);
  environment.write(tx => tx.openDatabase('items', { create: true }).put('key', 'before'));
  const snapshot = environment.begin();
  const oldView = snapshot.openDatabase('items');
  assert.throws(() => environment.write(tx => {
    tx.openDatabase('items').put('key', 'aborted');
    throw new Error('rollback');
  }), /rollback/);
  environment.write(tx => tx.openDatabase('items').put('key', 'after'));
  assert.equal(oldView.get('key').toString(), 'before');
  assert.equal(environment.read(tx => tx.openDatabase('items').get('key').toString()), 'after');
  assert.throws(() => environment.close(), /active transactions/);
  snapshot.abort();
  snapshot.abort();
  assert.throws(() => oldView.get('key'), /ended/);
});

test('read-only and transaction lifecycle errors stay in JavaScript', t => {
  const { directory, environment } = fixture(t);
  environment.write(tx => tx.openDatabase('items', { create: true }).put('key', 'value'));
  environment.read(tx => {
    assert.throws(() => tx.openDatabase('new', { create: true }), /read-only/);
    assert.throws(() => tx.openDatabase('items').put('key', 'new'), /read-only/);
  });
  const writer = environment.begin({ readOnly: false });
  assert.throws(() => environment.begin({ readOnly: false }), /already active/);
  writer.abort();
  assert.throws(() => environment.write(async () => 1), /synchronous/);
  assert.throws(() => environment.read(() => Promise.resolve(1)), /synchronous/);
  assert.throws(() => environment.read(tx => tx.openDatabase('bad\0name')), /NUL/);
  assert.throws(() => environment.read(tx => tx.openDatabase('items').put({}, 'x')), /read-only/);
  environment.close();
  assert.throws(() => environment.begin(), /closed/);
  const readOnly = new Environment(directory, { readOnly: true });
  try {
    assert.equal(readOnly.read(tx => tx.openDatabase('items').get('key').toString()), 'value');
    assert.throws(() => readOnly.write(() => {}), /read-only/);
  } finally { readOnly.close(); }
});

test('duplicate values are byte sorted and individually removable', t => {
  const { environment } = fixture(t);
  environment.write(tx => {
    const db = tx.openDatabase('tags', { kind: 'duplicates', create: true });
    db.put('key', 'z');
    db.put('key', 'a');
    db.put('key', 'a');
    assert.equal(db.get('key').toString(), 'a');
    assert.deepEqual(db.entries().map(([key, value]) => [key.toString(), value.toString()]), [['key', 'a'], ['key', 'z']]);
    assert.throws(() => db.put('key', 'a', { noDuplicateData: true }), { code: -30799 });
    assert.equal(db.delete('key', 'a'), true);
    assert.equal(db.delete('key', 'missing'), false);
  });
  assert.equal(environment.read(tx => tx.openDatabase('tags', { kind: 'duplicates' }).get('key').toString()), 'z');
});

test('IPv4 and IPv6 exact lookup, longest match, rollback and deletion', t => {
  const { environment } = fixture(t);
  environment.write(tx => {
    const v4 = tx.openDatabase('routes4', { kind: 'ipv4', create: true });
    v4.putPrefix('0.0.0.0/0', 'default');
    v4.putPrefix('192.0.2.0/24', 'example');
    v4.putPrefix('192.0.2.128/25', Buffer.from([0, 255]));
    const v6 = tx.openDatabase('routes6', { kind: 'ipv6', create: true });
    v6.putPrefix('2001:db8::/32', 'documentation');
    v6.putPrefix('2001:db8:1::/48', 'specific');
  });
  let retained;
  environment.read(tx => {
    const v4 = tx.openDatabase('routes4', { kind: 'ipv4' });
    assert.equal(v4.getPrefix('192.0.2.0/24').toString(), 'example');
    assert.equal(v4.getPrefix('198.51.100.0/24'), null);
    retained = v4.longestMatch('192.0.2.200');
    assert.deepEqual(retained, { prefix: '192.0.2.128/25', value: Buffer.from([0, 255]) });
    assert.equal(v4.longestMatch('203.0.113.1').prefix, '0.0.0.0/0');
    const v6 = tx.openDatabase('routes6', { kind: 'ipv6' });
    assert.deepEqual(v6.longestMatch('2001:db8:1::1234'), { prefix: '2001:db8:1::/48', value: Buffer.from('specific') });
    assert.equal(v6.longestMatch('::1'), null);
    assert.throws(() => v6.longestMatch('192.0.2.1'));
    assert.throws(() => v4.longestMatch('invalid'));
  });
  assert.deepEqual(retained.value, Buffer.from([0, 255]));
  assert.throws(() => environment.write(tx => {
    tx.openDatabase('routes4', { kind: 'ipv4' }).deletePrefix('192.0.2.128/25');
    throw new Error('rollback');
  }), /rollback/);
  environment.write(tx => {
    const v4 = tx.openDatabase('routes4', { kind: 'ipv4' });
    assert.equal(v4.deletePrefix('192.0.2.128/25'), true);
    assert.equal(v4.deletePrefix('192.0.2.128/25'), false);
  });
  assert.equal(environment.read(tx => tx.openDatabase('routes4', { kind: 'ipv4' }).longestMatch('192.0.2.200').prefix), '192.0.2.0/24');
});
