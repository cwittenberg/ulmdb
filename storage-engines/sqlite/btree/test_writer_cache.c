/* Focused scalar-cache admission/error/count checks; no database substitute
** is used by the SQL differential suites. Compile this standalone with cc. */
#include <assert.h>
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#define ULMDB_BTREE_NATIVE
#define BTREE_SCHEMA_VERSION 1
#define LUMO_LMDB_META_COUNTER 101
#define MDB_NOTFOUND (-30798)
#define MDB_CORRUPTED (-30796)
typedef uint64_t u64;
typedef uint32_t u32;
typedef unsigned char u8;
typedef struct { size_t mv_size; void *mv_data; } MDB_val;
typedef struct { void *txn; } Savepoint;
typedef struct {
  void *outerWriter; Savepoint *svp; unsigned dataDbi;
  u8 hasData, writerMetaValid, dataVersionValid;
  u64 writerCounter, dataVersionCounter;
  u32 writerSchema; int iDataVersion;
} Btree;
static unsigned reads, validations;
static int guardError, readError, missing;
static u32 storedSchema = 9;
static u64 storedCounter = 41;
static size_t wrongSize;
static u32 sqlite3Get4byte(const void *raw) {
  const u8 *p = raw;
  return ((u32)p[0] << 24) | ((u32)p[1] << 16) | ((u32)p[2] << 8) | p[3];
}
static u64 get8(const MDB_val *data) {
  const u8 *p = data->mv_data;
  return ((u64)sqlite3Get4byte(p) << 32) | sqlite3Get4byte(p + 4);
}
static int ubt_txn_validate(void *txn) {
  assert(txn); validations++; return guardError;
}
static int mdb_get(void *txn, unsigned dbi, MDB_val *key, MDB_val *out) {
  static u8 bytes[8];
  u64 value; unsigned tag = *(u8 *)key->mv_data; size_t size;
  (void)txn; (void)dbi; reads++;
  if (readError) return readError;
  if (missing) return MDB_NOTFOUND;
  assert(key->mv_size == 1);
  assert(tag == BTREE_SCHEMA_VERSION || tag == LUMO_LMDB_META_COUNTER);
  size = tag == BTREE_SCHEMA_VERSION ? 4 : 8;
  value = size == 4 ? storedSchema : storedCounter;
  for (size_t at = 0; at < size; at++) bytes[size - at - 1] = (u8)(value >> (at * 8));
  out->mv_data = bytes; out->mv_size = wrongSize ? wrongSize : size;
  return 0;
}
#include "writer_cache.c"
int main(void) {
  Savepoint sp = { (void *)1 };
  Btree b = {0}; int cookie = -1;
  b.outerWriter = (void *)2; b.svp = &sp; b.hasData = 1;
  assert(ulmdbWriterCacheActive(&b));
  for (unsigned i = 0; i < 10000; i++) {
    assert(ulmdbWriterVersion(&b, &cookie) == 0);
    assert(cookie == 9 && b.iDataVersion == 1 && b.dataVersionCounter == 41);
  }
  assert(reads == 2 && validations == 10000);
  /* A successful child merge preserves updated logical scalar values. */
  storedSchema = 10;
  ulmdbWriterMetaWritten(&b, 1, 4, 10, 0);
  assert(ulmdbWriterVersion(&b, &cookie) == 0 && cookie == 10 && reads == 2);
  /* Aborting/reopening a child must observe its restored metadata. */
  storedSchema = 9; ulmdbWriterCacheClear(&b);
  assert(ulmdbWriterVersion(&b, &cookie) == 0 && cookie == 9 && reads == 4);
  guardError = 5; cookie = -7;
  assert(ulmdbWriterVersion(&b, &cookie) == 5 && cookie == -7 && reads == 4);
  guardError = 0; ulmdbWriterCacheClear(&b); readError = 5;
  assert(ulmdbWriterVersion(&b, &cookie) == 5 && b.writerMetaValid == 0);
  readError = 0; wrongSize = 3;
  assert(ulmdbWriterVersion(&b, &cookie) == MDB_CORRUPTED && b.writerMetaValid == 0);
  wrongSize = 0; missing = 1;
  assert(ulmdbWriterVersion(&b, &cookie) == 0 && cookie == 0);
  assert(b.writerMetaValid == 3 && b.dataVersionCounter == 0);
  missing = 0; ulmdbWriterCacheClear(&b);
  assert(ulmdbWriterVersion(&b, &cookie) == 0 && cookie == 9);
  ulmdbWriterMetaWritten(&b, 101, 8, 42, 0);
  assert(ulmdbWriterVersion(&b, &cookie) == 0 && b.dataVersionCounter == 42);
  ulmdbWriterMetaWritten(&b, 1, 4, 999, 5);
  assert(!(b.writerMetaValid & 1));
  assert(ulmdbWriterVersion(&b, &cookie) == 0 && cookie == 9);
  ulmdbWriterMetaWritten(&b, 101, 4, 123, 0);
  assert(!(b.writerMetaValid & 2));
  b.outerWriter = NULL;
  assert(!ulmdbWriterCacheActive(&b));
  puts("writer cache: count, write-through, rollback reload, live guards and error admission: pass");
  return 0;
}
