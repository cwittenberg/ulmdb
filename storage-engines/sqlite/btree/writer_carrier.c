/* The native outer writer carries the immutable base and writer reservation.
** Every SQL mutation belongs to its child, including SQL's root savepoint.
** ROLLBACK TO that root replaces only the child; native ownership never moves. */
static int ulmdbBeginSqlWriter(Btree *p, MDB_txn **out) {
  MDB_txn *outer = NULL;
  int rc;
  if (p->outerWriter) return MDB_BAD_TXN;
  rc = mdb_txn_begin(p->env, NULL, 0, &outer);
  if (rc) return rc;
  rc = ulmdb_txn_base_checkpoint(outer, p->writerBase);
  if (!rc) rc = mdb_txn_begin(p->env, outer, 0, out);
  if (rc) { mdb_txn_abort(outer); return rc; }
  p->outerWriter = outer;
  return 0;
}

static void ulmdbAbortSqlTransaction(Btree *p, MDB_txn *txn) {
  if (p->outerWriter) {
    MDB_txn *outer = p->outerWriter;
    p->outerWriter = NULL;
    memset(p->writerBase, 0, sizeof(p->writerBase));
    /* The C ABI aborts any still-active descendants with their parent. */
    mdb_txn_abort(outer);
  } else if (txn) {
    mdb_txn_abort(txn);
  }
}

static int ulmdbCommitSqlWriter(Btree *p) {
  MDB_txn *outer = p->outerWriter;
  int rc;
  if (!outer || !p->svp || !p->svp->txn) return MDB_BAD_TXN;
  rc = mdb_txn_commit(p->svp->txn); /* Merge SQL changes; no publication. */
  p->svp->txn = NULL;
  if (rc) { ulmdbAbortSqlTransaction(p, NULL); return rc; }
  p->outerWriter = NULL;
  memset(p->writerBase, 0, sizeof(p->writerBase));
  return mdb_txn_commit(outer);    /* The single FULL durable publication. */
}

static int ulmdbRollbackRootSavepoint(Btree *p) {
  BtreeSavepoint *base;
  int rc;
  if (p->inTrans == SQLITE_TXN_NONE || ulmdbRealReader(p)) return SQLITE_OK;
  if (!p->outerWriter || !p->svp) return SQLITE_INTERNAL;
  /* SQLite already requested the schema-sensitive cursor trip policy.
  ** Preserve positions of surviving readers while unwinding nested writes. */
  rc = closeAllSavepoints(p, -1, 0, SQLITE_OK);
  if (rc) return error_map(rc);
  base = p->svp;
  assert(base && !base->prev && base->nSavepoint == -1);
  rc = invalidateCursors(p, base, SQLITE_ABORT_ROLLBACK,
                        (p->db->mDbFlags & DBFLAG_SchemaChange) == 0);
  if (rc) return error_map(rc);
  mdb_txn_abort(base->txn); /* Keep outerWriter and its exclusive base intact. */
  base->txn = NULL;
  p->hasData = 0;
  p->gtCacheValid = 0;
  rc = mdb_txn_begin(p->env, p->outerWriter, 0, &base->txn);
  if (!rc) rc = mdb_dbi_open(base->txn, "meta", 0, &p->dataDbi);
  if (!rc) { p->hasData = 1; return SQLITE_OK; }
  /* An explicit failure ends the SQL writer rather than retaining a broken
  ** child. Successful root rollback never executes this release path. */
  ulmdbAbortSqlTransaction(p, base->txn);
  sqlite3_free(base);
  p->svp = NULL;
  p->inTrans = SQLITE_TXN_NONE;
  lumoProcessWriteUnlock(p);
  return error_map(rc);
}
