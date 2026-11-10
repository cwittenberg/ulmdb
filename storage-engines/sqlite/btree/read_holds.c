/* SQLite may expose a borrowed ResultRow while another statement promotes
** this Btree to a writer. Keep that exact native reader alive until the last
** referencing SQLite cursor closes; no new snapshot or data copy is taken. */
struct UlmdbReadHold {
  MDB_txn *txn;
  unsigned refs;
  int retired;
  UlmdbReadHold *next;
};

static int ulmdbRealReader(Btree *p) {
  return p->inTrans == SQLITE_TXN_READ
      && (p->transType != LMDB_TRANSACTION_SERIALISE
          || (p->vfsFlags & SQLITE_OPEN_READONLY));
}

static int ulmdbAttachReader(BtCursor *cursor) {
  Btree *p = cursor->pBtree;
  UlmdbReadHold *hold;
  if (!ulmdbRealReader(p)) return SQLITE_OK;
  hold = p->readOwner;
  if (!hold) {
    hold = sqlite3MallocZero(sizeof(*hold));
    if (!hold) return SQLITE_NOMEM;
    hold->txn = p->svp->txn;
    p->readOwner = hold;
  }
  assert(hold->txn == p->svp->txn && !hold->retired);
  hold->refs++;
  cursor->readHold = hold;
  return SQLITE_OK;
}

static void ulmdbRetireReader(Btree *p, MDB_txn *txn) {
  UlmdbReadHold *hold = p->readOwner;
  assert(!hold || hold->txn == txn);
  if (hold && hold->txn == txn) {
    p->readOwner = NULL;
    if (hold->refs) {
      hold->retired = 1;
      hold->next = p->retiredReaders;
      p->retiredReaders = hold;
      return;
    }
    sqlite3_free(hold);
  }
  mdb_txn_abort(txn);
}

static void ulmdbReleaseReader(BtCursor *cursor) {
  Btree *p = cursor->pBtree;
  UlmdbReadHold *hold = cursor->readHold;
  UlmdbReadHold **link;
  if (!hold) return;
  assert(p && hold->refs > 0);
  cursor->readHold = NULL;
  hold->refs--;
  if (!hold->retired || hold->refs) return;
  for (link = &p->retiredReaders; *link; link = &(*link)->next) {
    if (*link == hold) { *link = hold->next; break; }
  }
  mdb_txn_abort(hold->txn);
  sqlite3_free(hold);
}

static void ulmdbParkReader(Btree *p) {
  UlmdbReadHold *hold = p->readOwner;
  if (hold) {
    assert(hold->refs == 0 && !hold->retired);
    p->readOwner = NULL;
    sqlite3_free(hold);
  }
}
