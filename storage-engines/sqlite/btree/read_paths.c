/* ULMDB B-tree prototype: materialize only the native cursor capability.
** A lazy exact hit owns its encoded key and borrows its value from the
** current immutable transaction.  Neither survives transaction invalidation.
** This fragment follows restoreCursor() in the generated amalgamation. */
static int ulmdbMaterializeReadCursor(BtCursor *pCur, int reposition) {
  Btree *p = pCur->pBtree;
  MDB_val key, data;
  int rc = restoreCursor(pCur, reposition);
  if (rc != SQLITE_OK || pCur->cursor) return rc;
  if (!p || !p->svp || !p->svp->txn) return SQLITE_ABORT;
  rc = get_table(p, pCur->rootPage, pCur->pKeyInfo ? get_table_index : 0,
                 &pCur->dbi);
  if (rc != SQLITE_OK) return rc;
  rc = mdb_cursor_open(p->svp->txn, pCur->dbi, &pCur->cursor);
  if (rc) return error_map(rc);
  pCur->txn = p->svp->txn;
  if (reposition && pCur->curValid && !pCur->cleared && !pCur->atEof) {
    key = pCur->curKey;
    rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_SET);
    if (rc) return error_map(rc);
    cursorCacheSet(pCur, &key, &data);
  }
  return SQLITE_OK;
}
