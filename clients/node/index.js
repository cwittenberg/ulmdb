'use strict';

const native = require('./build/ulmdb.node');
const kinds = Object.freeze({ kv: 0, duplicates: 1, ipv4: 4, ipv6: 6 });

function text(value, label) {
  if (typeof value !== 'string' || value.includes('\0')) throw new TypeError(`${label} must be a string without NUL bytes`);
  return value;
}
function bytes(value) {
  if (typeof value === 'string') return Buffer.from(value);
  if (Buffer.isBuffer(value)) return value;
  if (value instanceof Uint8Array) return Buffer.from(value.buffer, value.byteOffset, value.byteLength);
  throw new TypeError('Keys and values must be strings, Buffers, or Uint8Arrays');
}
function synchronous(result) {
  if (result && typeof result.then === 'function') {
    throw new TypeError('Transaction callbacks must be synchronous');
  }
  return result;
}

class Environment {
  #handle;
  #closed = false;
  #transactions = new Set();
  #writer = false;
  constructor(directory, { readOnly = false, maxDatabases = 128 } = {}) {
    if (!Number.isInteger(maxDatabases) || maxDatabases < 1 || maxDatabases > 4096) throw new RangeError('maxDatabases must be between 1 and 4096');
    this.readOnly = Boolean(readOnly);
    this.#handle = native.open(text(directory, 'directory'), this.readOnly, maxDatabases);
  }
  begin({ readOnly = true } = {}) {
    if (this.#closed) throw new Error('Environment is closed');
    if (!readOnly && this.readOnly) throw new Error('Environment is read-only');
    if (!readOnly && this.#writer) throw new Error('A write transaction is already active');
    const transaction = new Transaction(this, native.begin(this.#handle, Boolean(readOnly)), Boolean(readOnly), () => {
      this.#transactions.delete(transaction);
      if (!readOnly) this.#writer = false;
    });
    this.#transactions.add(transaction);
    if (!readOnly) this.#writer = true;
    return transaction;
  }
  read(callback) {
    const transaction = this.begin();
    try { return synchronous(callback(transaction)); }
    finally { transaction.abort(); }
  }
  write(callback) {
    const transaction = this.begin({ readOnly: false });
    try {
      const result = synchronous(callback(transaction));
      transaction.commit();
      return result;
    } catch (error) {
      transaction.abort();
      throw error;
    }
  }
  close() {
    if (this.#closed) return;
    if (this.#transactions.size) throw new Error('Finish active transactions before closing the environment');
    native.close(this.#handle);
    this.#closed = true;
  }
}

class Transaction {
  #handle;
  #done = false;
  #finish;
  constructor(environment, handle, readOnly, finish) {
    this.environment = environment;
    this.readOnly = readOnly;
    this.#handle = handle;
    this.#finish = finish;
  }
  _live(write = false) {
    if (this.#done) throw new Error('Transaction has ended');
    if (write && this.readOnly) throw new Error('Transaction is read-only');
    return this.#handle;
  }
  openDatabase(name, { kind = 'kv', create = false } = {}) {
    if (!Object.hasOwn(kinds, kind)) throw new TypeError(`Unknown database kind: ${kind}`);
    const handle = this._live(create);
    const dbi = native.database(handle, text(name, 'database name'), kinds[kind], Boolean(create));
    return new Database(this, dbi, kind);
  }
  commit() {
    const handle = this._live();
    this.#done = true;
    try { native.commit(handle); }
    finally { this.#finish(); }
  }
  abort() {
    if (this.#done) return;
    this.#done = true;
    try { native.abort(this.#handle); }
    finally { this.#finish(); }
  }
}

class Database {
  #transaction;
  #dbi;
  constructor(transaction, dbi, kind) {
    this.#transaction = transaction;
    this.#dbi = dbi;
    this.kind = kind;
  }
  #kv() {
    if (this.kind !== 'kv' && this.kind !== 'duplicates') throw new TypeError('Use prefix operations with an IP database');
  }
  #prefix() {
    if (this.kind !== 'ipv4' && this.kind !== 'ipv6') throw new TypeError('Prefix operations require an IP database');
  }
  get(key) {
    this.#kv();
    return native.get(this.#transaction._live(), this.#dbi, bytes(key));
  }
  put(key, value, { noOverwrite = false, noDuplicateData = false } = {}) {
    this.#kv();
    native.put(this.#transaction._live(true), this.#dbi, bytes(key), bytes(value), (noOverwrite ? 0x10 : 0) | (noDuplicateData ? 0x20 : 0));
  }
  delete(key, value) {
    this.#kv();
    return native.del(this.#transaction._live(true), this.#dbi, bytes(key), value === undefined ? null : bytes(value));
  }
  entries({ reverse = false } = {}) {
    this.#kv();
    return native.entries(this.#transaction._live(), this.#dbi, Boolean(reverse));
  }
  putPrefix(cidr, value) {
    this.#prefix();
    native.prefixPut(this.#transaction._live(true), this.#dbi, text(cidr, 'CIDR'), bytes(value));
  }
  getPrefix(cidr) {
    this.#prefix();
    return native.prefixGet(this.#transaction._live(), this.#dbi, text(cidr, 'CIDR'));
  }
  deletePrefix(cidr) {
    this.#prefix();
    return native.prefixDelete(this.#transaction._live(true), this.#dbi, text(cidr, 'CIDR'));
  }
  longestMatch(address) {
    this.#prefix();
    return native.longestMatch(this.#transaction._live(), this.#dbi, text(address, 'address'));
  }
}

module.exports = { Environment, Transaction, Database };
