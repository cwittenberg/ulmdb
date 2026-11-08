/// <reference types="node" />

export type Bytes = string | Buffer | Uint8Array;
export type DatabaseKind = 'kv' | 'duplicates' | 'ipv4' | 'ipv6';
export interface EnvironmentOptions {
  readOnly?: boolean;
  maxDatabases?: number;
}
export interface DatabaseOptions {
  kind?: DatabaseKind;
  create?: boolean;
}
export interface PrefixMatch {
  prefix: string;
  value: Buffer;
}

/** An embedded ULMDB environment. Call close() after finishing transactions. */
export class Environment {
  constructor(directory: string, options?: EnvironmentOptions);
  readonly readOnly: boolean;
  begin(options?: { readOnly?: boolean }): Transaction;
  /** Callback must be synchronous. All returned byte values are independent copies. */
  read<T>(callback: (transaction: Transaction) => T): T;
  /** Commits on success and aborts if the synchronous callback throws. */
  write<T>(callback: (transaction: Transaction) => T): T;
  close(): void;
}

/** Construct with Environment.begin(), read(), or write(). */
export class Transaction {
  private constructor();
  readonly environment: Environment;
  readonly readOnly: boolean;
  openDatabase(name: string, options?: DatabaseOptions): Database;
  commit(): void;
  abort(): void;
}

/** A database view belonging to one transaction; reopen it in later transactions. */
export class Database {
  private constructor();
  readonly kind: DatabaseKind;
  get(key: Bytes): Buffer | null;
  put(key: Bytes, value: Bytes, options?: { noOverwrite?: boolean; noDuplicateData?: boolean }): void;
  delete(key: Bytes, value?: Bytes): boolean;
  /** Returns copied entries in byte order; duplicate databases include every pair. */
  entries(options?: { reverse?: boolean }): Array<[Buffer, Buffer]>;
  putPrefix(cidr: string, value: Bytes): void;
  getPrefix(cidr: string): Buffer | null;
  deletePrefix(cidr: string): boolean;
  longestMatch(address: string): PrefixMatch | null;
}
