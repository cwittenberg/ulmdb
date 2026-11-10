/* lumo_ephemeral.c - in-memory backing for BTREE_SINGLE ephemeral btrees.
**
** See doc/context-about-ephemeral-tables.md for background.
**
** SQLite opens BTREE_SINGLE btrees (OP_OpenEphemeral and OP_OpenAutoindex) for
** transient tables, including DISTINCT, IN lists, automatic indexes, ORDER BY
** with a LIMIT, and rowid-keyed tables for materialised subqueries and
** RETURNING.  An LMDB-backed ephemeral index stores the whole record as the
** LMDB key, so a record longer than LMDB's maximum key size fails with
** SQLITE_TOOBIG.  Setting the LUMO_NOEPH environment variable stores
** BTREE_SINGLE btrees in LMDB.
**
** This file backs BTREE_SINGLE btrees with an in-process AVL tree instead.
** Index trees are ordered by sqlite3VdbeRecordCompare, which gives the same
** order as the LMDB index comparator; intkey trees are ordered by rowid.
**
** src/lumo_btree.c #includes this file immediately after the backend's
** btree.c, which defines EphNode, EphTree and EphCursor and declares the
** functions implemented here.
*/

/* Compare the stored record aRec against the unpacked probe pUnpacked, which
** the caller supplies.  Returns <0, 0 or >0 as aRec is less than, equal to or
** greater than the probe. */
static int ephCompareRec(EphTree *t, const unsigned char *aRec, u32 nA,
                         UnpackedRecord *pUnpacked) {
  return sqlite3VdbeRecordCompare((int)nA, aRec, pUnpacked);
}

static int ephH(EphNode *n) { return n ? n->height : 0; }
static void ephFix(EphNode *n) {
  int hl = ephH(n->left), hr = ephH(n->right);
  n->height = (hl > hr ? hl : hr) + 1;
}
static EphNode *ephRotL(EphNode *x) {
  EphNode *y = x->right; x->right = y->left; y->left = x;
  ephFix(x); ephFix(y); return y;
}
static EphNode *ephRotR(EphNode *x) {
  EphNode *y = x->left; x->left = y->right; y->right = x;
  ephFix(x); ephFix(y); return y;
}
static int ephBal(EphNode *n) { return n ? ephH(n->left) - ephH(n->right) : 0; }
static EphNode *ephRebalance(EphNode *n) {
  ephFix(n);
  int b = ephBal(n);
  if (b > 1) {
    if (ephBal(n->left) < 0) n->left = ephRotL(n->left);
    return ephRotR(n);
  }
  if (b < -1) {
    if (ephBal(n->right) > 0) n->right = ephRotR(n->right);
    return ephRotL(n);
  }
  return n;
}

static EphNode *ephNewNode(const unsigned char *rec, u32 nRec) {
  EphNode *n = (EphNode *)sqlite3Malloc(sizeof(EphNode) + nRec);
  if (!n) return NULL;
  n->left = n->right = NULL;
  n->height = 1;
  n->iKey = 0;
  n->nRec = nRec;
  if (nRec) memcpy(n->rec, rec, nRec);
  return n;
}

/* Insert record [rec,nRec) into the tree; pU is the same record in
** unpacked form, from the caller.  A record that compares equal to an
** existing node is not inserted: SQLite relies on this for IN lists and
** UNION, as its own btree keeps one entry per key.  *pInserted is 1 when a
** node was added, 0 for a duplicate, and -1 when allocation failed. */
static EphNode *ephInsertRec(EphTree *t, EphNode *n,
                             const unsigned char *rec, u32 nRec,
                             UnpackedRecord *pU, int *pInserted) {
  int c;
  if (!n) {
    n = ephNewNode(rec, nRec);
    *pInserted = n ? 1 : -1;
    return n;
  }
  /* compare stored node record vs probe: <0 node<probe, >0 node>probe */
  c = ephCompareRec(t, n->rec, n->nRec, pU);
  if (c < 0) {
    n->right = ephInsertRec(t, n->right, rec, nRec, pU, pInserted);
  } else if (c > 0) {
    n->left = ephInsertRec(t, n->left, rec, nRec, pU, pInserted);
  } else {
    /* equal record already present: keep the existing node */
    *pInserted = 0;
    return n;
  }
  return ephRebalance(n);
}

static EphNode *ephTakeMinNode(EphNode *n, EphNode **ppMin) {
  if (!n->left) {
    *ppMin = n;
    return n->right;
  }
  n->left = ephTakeMinNode(n->left, ppMin);
  return ephRebalance(n);
}

/* Delete the node equal to the unpacked probe pU.  Standard AVL delete using
** the same record comparator as insert, so ordering is consistent. */
static EphNode *ephDeleteRec(EphTree *t, EphNode *n, UnpackedRecord *pU,
                             int *pDeleted) {
  int c;
  if (!n) { *pDeleted = 0; return NULL; }
  c = ephCompareRec(t, n->rec, n->nRec, pU);  /* node vs probe */
  if (c < 0) {
    n->right = ephDeleteRec(t, n->right, pU, pDeleted);
  } else if (c > 0) {
    n->left = ephDeleteRec(t, n->left, pU, pDeleted);
  } else {
    *pDeleted = 1;
    if (!n->left || !n->right) {
      EphNode *child = n->left ? n->left : n->right;
      sqlite3_free(n);
      return child;
    } else {
      EphNode *repl = NULL;
      EphNode *right = ephTakeMinNode(n->right, &repl);
      repl->left = n->left;
      repl->right = right;
      sqlite3_free(n);
      return ephRebalance(repl);
    }
  }
  return ephRebalance(n);
}

/* Standard AVL delete by integer rowid, for intkey trees. */
static EphNode *ephDeleteInt(EphTree *t, EphNode *n, i64 iKey, int *pDeleted) {
  if (!n) { *pDeleted = 0; return NULL; }
  if (iKey < n->iKey) {
    n->left = ephDeleteInt(t, n->left, iKey, pDeleted);
  } else if (iKey > n->iKey) {
    n->right = ephDeleteInt(t, n->right, iKey, pDeleted);
  } else {
    *pDeleted = 1;
    if (!n->left || !n->right) {
      EphNode *child = n->left ? n->left : n->right;
      sqlite3_free(n);
      return child;
    } else {
      EphNode *repl = NULL;
      EphNode *right = ephTakeMinNode(n->right, &repl);
      repl->left = n->left;
      repl->right = right;
      sqlite3_free(n);
      return ephRebalance(repl);
    }
  }
  return ephRebalance(n);
}

/* Delete the node the cursor currently points at. */
static int ephDelete(EphCursor *c) {
  int deleted = 0;
  UnpackedRecord *pU;
  if (!c->cur) return SQLITE_OK;
  c->seekAfterDelete = 1;
  if (c->deletedRec) {
    sqlite3_free(c->deletedRec);
    c->deletedRec = NULL;
    c->nDeletedRec = 0;
  }
  if (c->tree->isIntKey) {
    c->deletedIKey = c->cur->iKey;
    c->tree->root = ephDeleteInt(c->tree, c->tree->root, c->cur->iKey, &deleted);
    if (deleted) c->tree->nEntry--;
    if (deleted) c->tree->version++;
    c->seenVersion = c->tree->version;
    c->cur = NULL;
    c->sp = 0;
    return SQLITE_OK;
  }
  pU = sqlite3VdbeAllocUnpackedRecord(c->tree->pKeyInfo);
  if (!pU) return SQLITE_NOMEM_BKPT;
  c->deletedRec = (unsigned char *)sqlite3Malloc(c->cur->nRec ? c->cur->nRec : 1);
  if (!c->deletedRec) {
    c->seekAfterDelete = 0;
    sqlite3DbFree(c->tree->pKeyInfo->db, pU);
    return SQLITE_NOMEM_BKPT;
  }
  c->nDeletedRec = c->cur->nRec;
  if (c->cur->nRec) memcpy(c->deletedRec, c->cur->rec, c->cur->nRec);
#if SQLITE_VERSION_NUMBER < 3051000
  sqlite3VdbeRecordUnpack(c->tree->pKeyInfo, (int)c->cur->nRec, c->cur->rec, pU);
#else
  sqlite3VdbeRecordUnpack((int)c->cur->nRec, c->cur->rec, pU);
#endif
  c->tree->root = ephDeleteRec(c->tree, c->tree->root, pU, &deleted);
  sqlite3DbFree(c->tree->pKeyInfo->db, pU);
  if (deleted) c->tree->nEntry--;
  if (deleted) c->tree->version++;
  c->seenVersion = c->tree->version;
  c->cur = NULL;
  c->sp = 0;
  return SQLITE_OK;
}

/* In-order iteration.  Nodes carry no parent pointer, so a cursor keeps an
** explicit path stack of ancestors-with-pending-right-subtree.  ephFirst /
** ephSeek build it; ephNext / ephPrev maintain it.  c->dir records whether the
** stack is currently arranged for forward (+1) or reverse (-1) iteration so
** that a Prev right after a forward-built seek (or vice versa) rebuilds the
** stack for the requested direction instead of walking a mismatched one. */


/* Rebuild the iteration stack so that c->cur is the just-popped node for a
** scan in direction dir (+1 forward, -1 reverse).  Descends from the root
** to c->cur, pushing each ancestor that comes after c->cur in the scan
** direction: those where the descent turns left going forward, or right
** going reverse.  O(log n).  Used when the scan direction changes, when the
** tree has changed since the stack was built, and after a seek from a
** deleted key. */
static int ephRebuildStack(EphCursor *c, int dir) {
  EphTree *t = c->tree;
  EphNode *target = c->cur, *n = t->root;
  UnpackedRecord *pU;
  c->sp = 0;
  if (!target) { c->dir = dir; return SQLITE_OK; }
  if (t->isIntKey) {
    while (n && n != target) {
      if (n->iKey < target->iKey) {
        if (dir < 0) c->stack[c->sp++] = n;
        n = n->right;
      } else if (n->iKey > target->iKey) {
        if (dir > 0) c->stack[c->sp++] = n;
        n = n->left;
      } else {
        break;
      }
    }
    c->seenVersion = t->version;
    c->dir = dir;
    return SQLITE_OK;
  }
  pU = sqlite3VdbeAllocUnpackedRecord(t->pKeyInfo);
  if (!pU) {
    /* Allocation failure: leave the stack empty rather than walking a
    ** mismatched stack, and report the failure to the caller. */
    c->dir = dir;
    return SQLITE_NOMEM_BKPT;
  }
#if SQLITE_VERSION_NUMBER < 3051000
  sqlite3VdbeRecordUnpack(t->pKeyInfo, (int)target->nRec, target->rec, pU);
#else
  sqlite3VdbeRecordUnpack((int)target->nRec, target->rec, pU);
#endif
  while (n && n != target) {
    int cmp = ephCompareRec(t, n->rec, n->nRec, pU); /* node vs target */
    /* equal records collapse on insert, so n is the target */
    if (cmp == 0) break;
    if (cmp < 0) {
      /* n < target: target is to the right.  Forward scan has already passed n,
      ** reverse scan still has n pending (n precedes target). */
      if (dir < 0) c->stack[c->sp++] = n;
      n = n->right;
    } else {
      /* n > target: target is to the left.  Forward scan still has n pending. */
      if (dir > 0) c->stack[c->sp++] = n;
      n = n->left;
    }
  }
  sqlite3DbFree(t->pKeyInfo->db, pU);
  c->dir = dir;
  c->seenVersion = t->version;
  return SQLITE_OK;
}

static void ephFirst(EphCursor *c, int *pRes) {
  EphNode *n = c->tree->root;
  c->sp = 0;
  c->dir = 1;
  c->seekAfterDelete = 0;
  while (n) { c->stack[c->sp++] = n; n = n->left; }
  if (c->sp == 0) { c->cur = NULL; *pRes = 1; return; }
  c->cur = c->stack[--c->sp];
  c->seenVersion = c->tree->version;
  *pRes = 0;
}

static void ephSeekFromDeletedInt(EphCursor *c, int dir, int *pRes) {
  EphNode *n = c->tree->root, *best = NULL;
  c->sp = 0;
  c->dir = dir;
  while (n) {
    if (dir > 0) {
      if (n->iKey > c->deletedIKey) { best = n; n = n->left; }
      else n = n->right;
    } else {
      if (n->iKey < c->deletedIKey) { best = n; n = n->right; }
      else n = n->left;
    }
  }
  c->cur = best;
  c->seekAfterDelete = 0;
  if (best) ephRebuildStack(c, dir);
  c->seenVersion = c->tree->version;
  *pRes = best ? 0 : 1;
}

static int ephSeekFromDeletedRec(EphCursor *c, int dir, int *pRes) {
  EphTree *t = c->tree;
  EphNode *n = t->root, *best = NULL;
  UnpackedRecord *pU;
  if (!c->deletedRec) { c->seekAfterDelete = 0; *pRes = 1; return SQLITE_OK; }
  pU = sqlite3VdbeAllocUnpackedRecord(t->pKeyInfo);
  if (!pU) return SQLITE_NOMEM_BKPT;
#if SQLITE_VERSION_NUMBER < 3051000
  sqlite3VdbeRecordUnpack(t->pKeyInfo, (int)c->nDeletedRec, c->deletedRec, pU);
#else
  sqlite3VdbeRecordUnpack((int)c->nDeletedRec, c->deletedRec, pU);
#endif
  c->sp = 0;
  c->dir = dir;
  while (n) {
    int cmp = ephCompareRec(t, n->rec, n->nRec, pU);
    if (dir > 0) {
      if (cmp > 0) { best = n; n = n->left; }
      else n = n->right;
    } else {
      if (cmp < 0) { best = n; n = n->right; }
      else n = n->left;
    }
  }
  sqlite3DbFree(t->pKeyInfo->db, pU);
  c->cur = best;
  c->seekAfterDelete = 0;
  if (best && ephRebuildStack(c, dir) != SQLITE_OK) return SQLITE_NOMEM_BKPT;
  c->seenVersion = c->tree->version;
  *pRes = best ? 0 : 1;
  return SQLITE_OK;
}

static int ephNext(EphCursor *c, int *pRes) {
  EphNode *n;
  if (c->seekAfterDelete) {
    if (c->tree->isIntKey) { ephSeekFromDeletedInt(c, 1, pRes); return SQLITE_OK; }
    return ephSeekFromDeletedRec(c, 1, pRes);
  }
  if (!c->cur) { *pRes = 1; return SQLITE_OK; }
  /* a changed tree, or a stack built for reverse iteration, needs a rebuild */
  if ((c->seenVersion != c->tree->version || c->dir != 1)
      && ephRebuildStack(c, 1) != SQLITE_OK) return SQLITE_NOMEM_BKPT;
  n = c->cur->right;
  if (n) {
    while (n) { c->stack[c->sp++] = n; n = n->left; }
  }
  if (c->sp == 0) { c->cur = NULL; *pRes = 1; return SQLITE_OK; }
  c->cur = c->stack[--c->sp];
  *pRes = 0;
  return SQLITE_OK;
}

/* Reverse iteration mirrors forward: stack holds ancestors with a pending
** LEFT subtree; descend right first. */
static void ephLast(EphCursor *c, int *pRes) {
  EphNode *n = c->tree->root;
  c->sp = 0;
  c->dir = -1;
  c->seekAfterDelete = 0;
  while (n) { c->stack[c->sp++] = n; n = n->right; }
  if (c->sp == 0) { c->cur = NULL; *pRes = 1; return; }
  c->cur = c->stack[--c->sp];
  c->seenVersion = c->tree->version;
  *pRes = 0;
}

static int ephPrev(EphCursor *c, int *pRes) {
  EphNode *n;
  if (c->seekAfterDelete) {
    if (c->tree->isIntKey) { ephSeekFromDeletedInt(c, -1, pRes); return SQLITE_OK; }
    return ephSeekFromDeletedRec(c, -1, pRes);
  }
  if (!c->cur) { *pRes = 1; return SQLITE_OK; }
  /* a changed tree, or a forward/seek position, needs the stack rebuilt */
  if ((c->seenVersion != c->tree->version || c->dir != -1)
      && ephRebuildStack(c, -1) != SQLITE_OK) return SQLITE_NOMEM_BKPT;
  n = c->cur->left;
  if (n) {
    while (n) { c->stack[c->sp++] = n; n = n->right; }
  }
  if (c->sp == 0) { c->cur = NULL; *pRes = 1; return SQLITE_OK; }
  c->cur = c->stack[--c->sp];
  *pRes = 0;
  return SQLITE_OK;
}

static i64 ephCount(EphTree *t) { return t ? t->nEntry : 0; }

/* Positioned seek used by sqlite3BtreeIndexMoveto for equality seeks
** (OP_Found, OP_NotFound, OP_NoConflict, OP_IdxDelete) and range seeks
** (OP_SeekGE/GT/LE/LT).
**
** Descends from the root toward probe pU, leaving the cursor positioned on the
** nearest node and *pRes set to the signed comparison of that node against the
** probe (SQLite's Moveto convention):
**   0  exact match (cursor on it)
**  <0  cursor on a node < probe
**  >0  cursor on a node > probe
** The forward iteration stack is built during the descent so a following
** sqlite3BtreeNext steps to the in-order successor; a following Prev rebuilds
** the reverse stack on demand (see ephPrev).  pU->eqSeen is set when an exact
** match exists, which the VDBE's eqOnly fast-path requires.
**
** On an empty tree the cursor is left NULL and *pRes = -1 (the caller treats a
** negative result on an empty cursor via sqlite3BtreeEof). */
static int ephSeek(EphCursor *c, UnpackedRecord *pU, int *pRes) {
  EphTree *t = c->tree;
  EphNode *n = t->root, *last = NULL;
  int cLast = -1;
  c->sp = 0;
  c->dir = 1;                       /* forward stack built below */
  c->seekAfterDelete = 0;
  while (n) {
    int cmp = ephCompareRec(t, n->rec, n->nRec, pU);  /* node vs probe */
    last = n;
    cLast = cmp;
    if (cmp == 0) {
      /* Exact match.  Leave the forward stack as the ancestors with pending
      ** right subtrees so ephNext advances correctly past this node. */
      pU->eqSeen = 1;
      c->cur = n;
      c->seenVersion = t->version;
      *pRes = 0;
      return SQLITE_OK;
    }
    if (cmp < 0) {
      /* node < probe: probe is to the right; node is behind us in forward order
      ** so it is NOT a pending successor.  Do not push. */
      n = n->right;
    } else {
      /* node > probe: probe is to the left; node is a pending in-order
      ** successor, pushed for ephNext. */
      c->stack[c->sp++] = n;
      n = n->left;
    }
  }
  if (!last) {                      /* empty tree */
    c->cur = NULL;
    c->seenVersion = t->version;
    *pRes = -1;
    return SQLITE_OK;
  }
  c->cur = last;
  c->seenVersion = t->version;
  *pRes = cLast;                    /* signed comparison of landed node vs probe */
  /* If we landed on a node greater than the probe (final step was a left turn),
  ** that node is itself the in-order successor and we pushed it above; pop it so
  ** the stack holds only the strict ancestors-with-pending-right of c->cur,
  ** matching the ephNext invariant. */
  if (cLast > 0 && c->sp > 0 && c->stack[c->sp - 1] == last) c->sp--;
  return SQLITE_OK;
}

/* Intkey (rowid-keyed) trees.  An ephemeral btree opened without a KeyInfo is
** an ordinary rowid table; SQLite uses one for RETURNING results, materialised
** subqueries and views, and other transient rowid-keyed storage.
*/

static EphNode *ephInsertInt(EphTree *t, EphNode *n, i64 iKey,
                             const unsigned char *rec, u32 nRec, int *pIns) {
  if (!n) { n = ephNewNode(rec, nRec); *pIns = n ? 1 : -1; if (n) n->iKey = iKey; return n; }
  if (iKey < n->iKey) {
    n->left = ephInsertInt(t, n->left, iKey, rec, nRec, pIns);
  } else if (iKey > n->iKey) {
    n->right = ephInsertInt(t, n->right, iKey, rec, nRec, pIns);
  } else {
    /* rowid already present: replace the payload in place (INSERT OR REPLACE /
    ** rowid reuse).  Reallocate the node if the new payload is larger. */
    *pIns = 0;
    if (nRec <= n->nRec) {
      if (nRec) memcpy(n->rec, rec, nRec);
      n->nRec = nRec;
      return n;
    } else {
      EphNode *m = ephNewNode(rec, nRec);
      if (!m) { *pIns = -1; return n; }   /* OOM: keep old node */
      m->iKey = iKey;
      m->left = n->left; m->right = n->right; m->height = n->height;
      sqlite3_free(n);
      return m;
    }
  }
  return ephRebalance(n);
}

/* Insert (or replace) a row keyed by integer rowid into an intkey tree. */
static int ephInsertIntKey(BtCursor *pCur, i64 iKey,
                           const unsigned char *rec, u32 nRec) {
  EphCursor *c = (EphCursor *)pCur->ephCur;
  EphTree *t = c->tree;
  int inserted = 0;
  c->seekAfterDelete = 0;
  t->isIntKey = 1;
  t->root = ephInsertInt(t, t->root, iKey, rec, nRec, &inserted);
  if (inserted > 0) t->nEntry++;
  t->version++;
  c->seenVersion = t->version;
  return inserted < 0 ? SQLITE_NOMEM_BKPT : SQLITE_OK;
}

/* Seek an intkey tree to rowid iKey.  Leaves the cursor on the nearest node and
** sets *pRes to sign(landed.iKey - iKey) per the Moveto convention, building
** the forward iteration stack for a following sqlite3BtreeNext. */
static int ephSeekInt(EphCursor *c, i64 iKey, int *pRes) {
  EphTree *t = c->tree;
  EphNode *n = t->root, *last = NULL;
  int cLast = -1;
  c->sp = 0;
  c->dir = 1;
  c->seekAfterDelete = 0;
  while (n) {
    last = n;
    if (iKey == n->iKey) { c->cur = n; c->seenVersion = t->version; *pRes = 0; return SQLITE_OK; }
    if (n->iKey < iKey) {              /* node < probe: not a pending successor */
      cLast = -1;
      n = n->right;
    } else {                          /* node > probe: pending in-order successor */
      cLast = 1;
      c->stack[c->sp++] = n;
      n = n->left;
    }
  }
  if (!last) { c->cur = NULL; c->seenVersion = t->version; *pRes = -1; return SQLITE_OK; }
  c->cur = last;
  c->seenVersion = t->version;
  *pRes = cLast;
  if (cLast > 0 && c->sp > 0 && c->stack[c->sp - 1] == last) c->sp--;
  return SQLITE_OK;
}

/* Rowid of the current node, or 0 when the cursor is not on a row. */
static i64 ephIntKey(EphCursor *c) {
  return c->cur ? c->cur->iKey : 0;
}

/* Cursor open/close glue used by sqlite3BtreeCursor / CloseCursor. */
static int ephCursorOpen(Btree *p, BtCursor *pCur, struct KeyInfo *pKeyInfo) {
  EphTree *t = (EphTree *)p->pEph;
  EphCursor *c;
  if (!t) {
    t = (EphTree *)sqlite3MallocZero(sizeof(EphTree));
    if (!t) return SQLITE_NOMEM_BKPT;
    t->pKeyInfo = pKeyInfo;
    t->isIntKey = (pKeyInfo == NULL);   /* no KeyInfo => rowid-keyed table */
    p->pEph = t;
  }
  if (!t->pKeyInfo) t->pKeyInfo = pKeyInfo;
  c = (EphCursor *)sqlite3MallocZero(sizeof(EphCursor));
  if (!c) return SQLITE_NOMEM_BKPT;
  c->tree = t;
  pCur->ephCur = c;
  return SQLITE_OK;
}

static void ephCursorClose(BtCursor *pCur) {
  if (pCur->ephCur) {
    EphCursor *c = (EphCursor *)pCur->ephCur;
    if (c->deletedRec) sqlite3_free(c->deletedRec);
    sqlite3_free(c);
    pCur->ephCur = NULL;
  }
}

/* Free the tree when its btree closes. */
static void ephTreeFree(Btree *p) {
  if (p->pEph) {
    ephClear((EphTree *)p->pEph);
    sqlite3_free(p->pEph);
    p->pEph = NULL;
  }
}

/* Insert from sqlite3BtreeInsert: rec/nRec is the record from pX->pKey, and
** pU is its unpacked form, built or borrowed by the caller. */
static int ephInsert(BtCursor *pCur, const unsigned char *rec, u32 nRec,
                     UnpackedRecord *pU) {
  EphCursor *c = (EphCursor *)pCur->ephCur;
  EphTree *t = c->tree;
  int inserted = 0;
  c->seekAfterDelete = 0;
  t->root = ephInsertRec(t, t->root, rec, nRec, pU, &inserted);
  if (inserted > 0) t->nEntry++;
  t->version++;
  c->seenVersion = t->version;
  return inserted < 0 ? SQLITE_NOMEM_BKPT : SQLITE_OK;
}

/* Payload accessors: the current node owns the record bytes. */
static const unsigned char *ephPayload(BtCursor *pCur, u32 *pnRec) {
  EphCursor *c = (EphCursor *)pCur->ephCur;
  if (!c->cur) { *pnRec = 0; return NULL; }
  *pnRec = c->cur->nRec;
  return c->cur->rec;
}

/* Free the whole tree (ClearTable / close). */
static void ephFreeNode(EphNode *n) {
  if (!n) return;
  ephFreeNode(n->left);
  ephFreeNode(n->right);
  sqlite3_free(n);
}
static void ephClear(EphTree *t) {
  if (!t) return;
  ephFreeNode(t->root);
  t->root = NULL;
  t->nEntry = 0;
  t->version++;
}
