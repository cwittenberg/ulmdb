/* Only the process-owned native profile can hold these two scalar metadata
** values across statements. Successful child merges preserve their view;
** every abort/reopen drops it. No read snapshot or returned row is cached. */
#ifdef ULMDB_BTREE_NATIVE
static void ulmdbWriterCacheClear(Btree *p) {
  p->writerMetaValid = 0;
}

static int ulmdbWriterCacheActive(Btree *p) {
  return p->outerWriter && p->svp && p->svp->txn && p->hasData;
}

static int ulmdbWriterMeta(Btree *p, unsigned int idx, u64 *value) {
  unsigned char tag = (unsigned char)idx;
  unsigned mask = tag == BTREE_SCHEMA_VERSION ? 1 : 2;
  MDB_val key = { 1, &tag }, data;
  int rc;
  assert(tag == BTREE_SCHEMA_VERSION || tag == LUMO_LMDB_META_COUNTER);
  if (p->writerMetaValid & mask) {
    *value = mask == 1 ? p->writerSchema : p->writerCounter;
    return 0;
  }
  rc = mdb_get(p->svp->txn, p->dataDbi, &key, &data);
  if (rc == MDB_NOTFOUND) *value = 0;
  else if (rc) return rc;
  else {
    if (data.mv_size != (mask == 1 ? 4 : 8)) return MDB_CORRUPTED;
    *value = mask == 1 ? sqlite3Get4byte(data.mv_data) : get8(&data);
  }
  if (mask == 1) p->writerSchema = (u32)*value;
  else p->writerCounter = *value;
  p->writerMetaValid |= mask;
  return 0;
}

static void ulmdbWriterMetaWritten(Btree *p, unsigned int idx,
                                    unsigned size, u64 value, int rc) {
  unsigned char tag = (unsigned char)idx;
  unsigned mask;
  if (!ulmdbWriterCacheActive(p)) return;
  if (tag == BTREE_SCHEMA_VERSION) mask = 1;
  else if (tag == LUMO_LMDB_META_COUNTER) mask = 2;
  else return;
  p->writerMetaValid &= ~mask;
  if (rc || size != (mask == 1 ? 4 : 8)) return;
  if (mask == 1) p->writerSchema = (u32)value;
  else p->writerCounter = value;
  p->writerMetaValid |= mask;
}

static int ulmdbWriterVersion(Btree *p, int *schema) {
  u64 counter, cookie;
  int rc = ubt_txn_validate(p->svp->txn);
  if (!rc) rc = ulmdbWriterMeta(p, LUMO_LMDB_META_COUNTER, &counter);
  if (!rc && schema) rc = ulmdbWriterMeta(p, BTREE_SCHEMA_VERSION, &cookie);
  if (rc) return rc;
  if (!p->dataVersionValid) {
    p->dataVersionCounter = counter;
    p->dataVersionValid = 1;
    p->iDataVersion = 1;
  } else if (counter != p->dataVersionCounter) {
    p->dataVersionCounter = counter;
    p->iDataVersion++;
  }
  if (schema) *schema = (u32)cookie;
  return 0;
}
#else
#define ulmdbWriterCacheClear(p) ((void)0)
#endif
