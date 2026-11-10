/* ULMDB isolated B-tree prototype: unsupported page-copy APIs fail closed. */
struct sqlite3_backup { int unused; };
sqlite3_backup *sqlite3_backup_init(sqlite3 *dst, const char *dn,
                                  sqlite3 *src, const char *sn) {
  (void)dn; (void)src; (void)sn;
  if (dst) sqlite3ErrorWithMsg(dst, SQLITE_ERROR,
                              "ULMDB B-tree prototype: backup is unavailable");
  return NULL;
}
int sqlite3_backup_step(sqlite3_backup *p, int n) { (void)p; (void)n; return SQLITE_MISUSE; }
int sqlite3_backup_finish(sqlite3_backup *p) { (void)p; return SQLITE_OK; }
int sqlite3_backup_remaining(sqlite3_backup *p) { (void)p; return 0; }
int sqlite3_backup_pagecount(sqlite3_backup *p) { (void)p; return 0; }

/* Keep the public symbols available to language runtimes compiled against
** SQLite's serialization API, while refusing this different disk format. */
SQLITE_API unsigned char *sqlite3_serialize(sqlite3 *db, const char *schema,
                                         sqlite3_int64 *size, unsigned flags) {
  (void)schema; (void)flags;
  if (size) *size = -1;
  if (db) {
    sqlite3_mutex_enter(db->mutex);
    sqlite3ErrorWithMsg(db, SQLITE_ERROR,
                      "ULMDB B-tree prototype: serialize is unavailable");
    sqlite3_mutex_leave(db->mutex);
  }
  return NULL;
}

SQLITE_API int sqlite3_deserialize(sqlite3 *db, const char *schema,
                                  unsigned char *data, sqlite3_int64 size,
                                  sqlite3_int64 capacity, unsigned flags) {
  (void)schema; (void)size; (void)capacity;
  /* The upstream contract transfers FREEONCLOSE ownership even on failure. */
  if (flags & SQLITE_DESERIALIZE_FREEONCLOSE) sqlite3_free(data);
  if (!db) return SQLITE_MISUSE;
  sqlite3_mutex_enter(db->mutex);
  sqlite3ErrorWithMsg(db, SQLITE_ERROR,
                    "ULMDB B-tree prototype: deserialize is unavailable");
  sqlite3_mutex_leave(db->mutex);
  return SQLITE_ERROR;
}
