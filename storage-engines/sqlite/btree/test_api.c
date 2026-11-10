#include "sqlite3.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>

/* Track the specific caller buffer independently of SQLite's error-message
** allocations, proving the deserialize failure ownership contract. */
static sqlite3_mem_methods original;
static void *tracked;
static int frees;
static void *allocate(int n) { return original.xMalloc(n); }
static void release(void *p) { if (p == tracked) frees++; original.xFree(p); }
static void *resize(void *p, int n) { return original.xRealloc(p, n); }
static int size_of(void *p) { return original.xSize(p); }
static int roundup(int n) { return original.xRoundup(n); }
static int initialize(void *p) { (void)p; return original.xInit(original.pAppData); }
static void shutdown(void *p) { (void)p; original.xShutdown(original.pAppData); }

int main(void) {
  sqlite3 *db = NULL;
  sqlite3_mem_methods wrapped;
  sqlite3_int64 bytes = 123;
  sqlite3_stmt *pending = NULL;
  const unsigned char *borrowed;
  assert(sqlite3_config(SQLITE_CONFIG_GETMALLOC, &original) == SQLITE_OK);
  wrapped = (sqlite3_mem_methods){allocate, release, resize, size_of, roundup,
                                 initialize, shutdown, NULL};
  assert(sqlite3_config(SQLITE_CONFIG_MALLOC, &wrapped) == SQLITE_OK);
  assert(sqlite3_open(":memory:", &db) == SQLITE_OK);
  assert(sqlite3_exec(db, "CREATE TABLE preserved(id INTEGER PRIMARY KEY,value TEXT DEFAULT 'base'); INSERT INTO preserved(id) VALUES(7),(8);", NULL, NULL, NULL) == SQLITE_OK);
  tracked = sqlite3_malloc(32); assert(tracked); frees = 0;
  assert(sqlite3_deserialize(db, "main", tracked, 32, 32,
                            SQLITE_DESERIALIZE_FREEONCLOSE) == SQLITE_ERROR);
  assert(frees == 1);
  assert(strstr(sqlite3_errmsg(db), "deserialize is unavailable"));
  tracked = sqlite3_malloc(32); assert(tracked); frees = 0;
  assert(sqlite3_deserialize(db, "main", tracked, 32, 32, 0) == SQLITE_ERROR);
  assert(frees == 0);
  sqlite3_free(tracked); assert(frees == 1); tracked = NULL;
  assert(sqlite3_serialize(db, "main", &bytes, 0) == NULL && bytes == -1);
  assert(strstr(sqlite3_errmsg(db), "serialize is unavailable"));
  assert(sqlite3_exec(db, "SELECT * FROM preserved;", NULL, NULL, NULL) == SQLITE_OK);
  assert(sqlite3_extended_result_codes(db, 1) == SQLITE_OK);
  assert(sqlite3_exec(db, "SAVEPOINT pending_root; UPDATE preserved SET value='pending borrowed bytes';", NULL, NULL, NULL) == SQLITE_OK);
  assert(sqlite3_prepare_v2(db, "SELECT value FROM preserved WHERE id>=7 ORDER BY id", -1, &pending, NULL) == SQLITE_OK);
  assert(sqlite3_step(pending) == SQLITE_ROW);
  borrowed = sqlite3_column_blob(pending, 0);
  assert(sqlite3_column_bytes(pending, 0) == 22);
  assert(memcmp(borrowed, "pending borrowed bytes", 22) == 0);
  assert(sqlite3_exec(db, "ROLLBACK TO pending_root;", NULL, NULL, NULL) == SQLITE_OK);
  assert(memcmp(borrowed, "pending borrowed bytes", 22) == 0);
  /* SQLite preserves read cursors on a rollback without schema changes.
  ** The following row is read from the restored SQL child. */
  assert(sqlite3_step(pending) == SQLITE_ROW);
  assert(sqlite3_column_bytes(pending, 0) == 4);
  assert(memcmp(sqlite3_column_blob(pending, 0), "base", 4) == 0);
  assert(sqlite3_step(pending) == SQLITE_DONE);
  sqlite3_finalize(pending);
  assert(sqlite3_exec(db, "RELEASE pending_root; SELECT * FROM preserved;", NULL, NULL, NULL) == SQLITE_OK);
  assert(sqlite3_exec(db, "SAVEPOINT schema_root; UPDATE preserved SET value='pending borrowed bytes';", NULL, NULL, NULL) == SQLITE_OK);
  assert(sqlite3_prepare_v2(db, "SELECT value FROM preserved WHERE id>=7 ORDER BY id", -1, &pending, NULL) == SQLITE_OK);
  assert(sqlite3_step(pending) == SQLITE_ROW);
  borrowed = sqlite3_column_blob(pending, 0);
  assert(sqlite3_column_bytes(pending, 0) == 22);
  assert(sqlite3_exec(db, "CREATE TABLE rollback_ddl(id INTEGER PRIMARY KEY); ROLLBACK TO schema_root;", NULL, NULL, NULL) == SQLITE_OK);
  assert(memcmp(borrowed, "pending borrowed bytes", 22) == 0);
  assert(sqlite3_step(pending) == SQLITE_ABORT_ROLLBACK);
  sqlite3_finalize(pending);
  assert(sqlite3_exec(db, "RELEASE schema_root;", NULL, NULL, NULL) == SQLITE_OK);
  assert(sqlite3_close(db) == SQLITE_OK);
  {
    char directory[] = "/tmp/ulmdb-btree-no-create-XXXXXX";
    char filename[256], sidecar[272];
    struct stat state;
    assert(mkdtemp(directory));
    snprintf(filename, sizeof filename, "%s/missing.db", directory);
    assert(sqlite3_open_v2(filename, &db, SQLITE_OPEN_READWRITE, NULL) == SQLITE_CANTOPEN);
    assert(sqlite3_close(db) == SQLITE_OK);
    assert(lstat(filename, &state) != 0);
    snprintf(sidecar, sizeof sidecar, "%s.ulmdb", filename);
    assert(lstat(sidecar, &state) != 0);
    assert(rmdir(directory) == 0);
  }
  sqlite3_shutdown();
  puts("serialize/deserialise refusal and buffer ownership: pass");
  return 0;
}
