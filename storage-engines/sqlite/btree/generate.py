#!/usr/bin/env python3
"""Generate an isolated SQLite 3.54.1 amalgamation with the prototype backend.

The compiler/parser/VM remain the verified upstream amalgamation.  Backend
sources are vendored verbatim; the explicit, checked substitutions below are
the ULMDB port.  A mismatched source fails before producing any output.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
SQLITE_SHA256 = "0c657666d819f261cc4007526ad57e1669de9949df7a8ea847aa284a2bf0ee52"


def replace(text, before, after, count=1):
    actual = text.count(before)
    if actual != count:
        raise ValueError(f"port anchor count {actual}, expected {count}: {before[:90]!r}")
    return text.replace(before, after)


def backend():
    b = (HERE / "lumosql/btree.c").read_text()
    b = replace(b, '#include "lmdb.h"', '#ifdef ULMDB_BTREE_NATIVE\n#include "native_backend.h"\n#endif\n#include "ulmdb.h"\n#include <ftw.h>\n#define BTREE_LUMO_EXTENSIONS 0\n#undef LUMO_LMDB_SYNC')
    # Signed rowids sort bytewise, without callback state or machine endianness.
    b = replace(b, 'return res | (u64)sqlite3Get4byte(ptr + 4);',
                'return (res | (u64)sqlite3Get4byte(ptr + 4)) ^ (((u64)1) << 63);')
    b = replace(b, 'static void put8(MDB_val *d, unsigned char *b, u64 v) {\n  sqlite3Put4byte',
                'static void put8(MDB_val *d, unsigned char *b, u64 v) {\n  v ^= ((u64)1) << 63;\n  sqlite3Put4byte')
    b = replace(b, 'static int get_meta_64(', 'static u64 get_meta_64(')
    b = replace(b, 'struct Btree {',
        'typedef struct UlmdbReadHold UlmdbReadHold;\nstruct Btree {\n'
        '  UlmdbReadHold *readOwner;\n  UlmdbReadHold *retiredReaders;')
    b = replace(b, '  u8 inTrans;\n', '  u8 inTrans;\n  u32 heldReadSchema;\n')
    b = replace(b, '  MDB_env *env;\n', '  MDB_env *env;\n  pid_t creatorPid;\n')
    b = replace(b, '  u8 wrFlag;\n',
        '  u8 wrFlag;\n  u8 lazyRead;\n  unsigned char logicalKey[8];\n'
        '  UlmdbReadHold *readHold;\n')
    insert = b.index('/* Record the row the LMDB cursor')
    b = b[:insert] + (HERE / 'read_holds.c').read_text() + '\n' + b[insert:]
    # Remove the upstream key[1] comparator protocol completely.  The C ABI
    # supplies one MDB_val per operand, and may compare stored keys at commit.
    start = b.index('/* Compare rowids as signed integers')
    end = b.index('/* Spill-to-value record format', start)
    b = b[:start] + 'static int lumoNeedsSpill(struct KeyInfo *pKeyInfo);\n\n' + b[end:]
    start = b.index('  if (tableFlags & get_table_index) {', b.index('static int get_table('))
    end = b.index('  if (rc == SQLITE_OK) {', start)
    b = b[:start] + '  /* XOR-BE64 primary keys use the default bytewise comparator. */\n' + b[end:]
    b = replace(b, '  if (iTable < 1) return SQLITE_CORRUPT_BKPT;',
        '  if (tableFlags & get_table_index) {\n'
        '    sqlite3ErrorWithMsg(p->db, SQLITE_ERROR, "ULMDB B-tree prototype: persistent indexes are unavailable");\n'
        '    return SQLITE_ERROR;\n  }\n'
        '  if (iTable < 1) return SQLITE_CORRUPT_BKPT;')
    start = b.index('  MDB_val key[2], data;\n  int rc;\n  if (pCur->pBtree->pEph) {', b.index('int sqlite3BtreeIndexMoveto('))
    end = b.index('\n/* Entry point used by SQLite before 3.37', start)
    b = b[:start] + '''  if (pCur->pBtree->pEph) {
    int rc = ephSeek((EphCursor *)pCur->ephCur, pIdxKey, pRes);
    pCur->atEof = (((EphCursor *)pCur->ephCur)->cur == NULL);
    return rc;
  }
  return SQLITE_ERROR; /* No persistent record comparator is registered. */
}
''' + b[end:]
    b = replace(b, '(p->flags & BTREE_SINGLE) && getenv("LUMO_NOEPH") == NULL',
                '(p->flags & BTREE_SINGLE)')
    # BTREE_SINGLE is VM scratch, entirely owned by EphTree.  It needs no
    # filesystem/environment/snapshot and never joins durable transactions.
    b = replace(b, '  mdbFlags = MDB_NOTLS;', '''  if (flags & BTREE_SINGLE) {
    p = sqlite3MallocZero(sizeof(Btree) + 1);
    if (!p) return SQLITE_NOMEM;
    p->db = db; p->pVfs = pVfs; p->flags = flags;
    p->vfsFlags = vfsFlags; p->isTemp = 1; p->isMemdb = 1;
    *ppBtree = p;
    return SQLITE_OK;
  }
  mdbFlags = MDB_NOTLS;''')
    b = replace(b, '  pCur->txn = p->svp->txn;',
                '  pCur->txn = p->svp ? p->svp->txn : NULL;')
    b = replace(b, '  rc = mdb_cursor_open(p->svp->txn, pCur->dbi, &pCur->cursor);\n  if (rc) return error_map(rc);',
        '  rc = mdb_cursor_open(p->svp->txn, pCur->dbi, &pCur->cursor);\n'
        '  if (rc) return error_map(rc);\n  pCur->txn = p->svp->txn;')
    b = replace(b, 'int sqlite3BtreeClose(Btree *p) {', '''int sqlite3BtreeClose(Btree *p) {
  if (p->flags & BTREE_SINGLE) {
    if (p->pEph) ephTreeFree(p);
    sqlite3_free(p);
    return SQLITE_OK;
  }''')
    b = replace(b, '  /* sqlite3BtreeCopyFile leaves no env when it cannot reopen the database */', '''  if (p->flags & BTREE_SINGLE) {
    p->inTrans = wrFlag ? SQLITE_TXN_WRITE : SQLITE_TXN_READ;
    if (pSchemaVersion) *pSchemaVersion = 0;
    return SQLITE_OK;
  }
  /* sqlite3BtreeCopyFile leaves no env when it cannot reopen the database */''')
    # The active native reader pins an immutable schema and commit epoch.
    # New begin/renew still reads both metadata values; writer/DDL paths keep
    # their existing metadata and savepoint checks.
    b = replace(b, '  if (p->inTrans != SQLITE_TXN_NONE) {\n    /* we already have a read-only transaction;',
        '  if (p->inTrans == SQLITE_TXN_READ && !wrFlag\n'
        '      && p->transType != LMDB_TRANSACTION_SERIALISE) {\n'
        '    if (pSchemaVersion) *pSchemaVersion = p->heldReadSchema;\n'
        '    return SQLITE_OK;\n  }\n'
        '  if (p->inTrans != SQLITE_TXN_NONE) {\n    /* we already have a read-only transaction;')
    b = replace(b, '  if (pSchemaVersion)\n    *pSchemaVersion = get_meta_32(p, BTREE_SCHEMA_VERSION);',
        '  if (p->inTrans == SQLITE_TXN_READ && p->transType != LMDB_TRANSACTION_SERIALISE) {\n'
        '    p->heldReadSchema = get_meta_32(p, BTREE_SCHEMA_VERSION);\n'
        '    if (pSchemaVersion) *pSchemaVersion = p->heldReadSchema;\n'
        '  } else if (pSchemaVersion)\n    *pSchemaVersion = get_meta_32(p, BTREE_SCHEMA_VERSION);')
    for signature in ['int sqlite3BtreeCommitPhaseOne(Btree *p, const char *zSuperJrnl) {',
                      'int sqlite3BtreeCommitPhaseTwo(Btree *p, int bCleanup) {',
                      'int sqlite3BtreeRollback(Btree *p, int tripCode, int writeOnly) {']:
        b = replace(b, signature, signature + '\n  if (p->flags & BTREE_SINGLE) { p->inTrans = SQLITE_TXN_NONE; return SQLITE_OK; }')
    b = replace(b, 'int sqlite3BtreeBeginStmt(Btree *p, int iStatement) {',
                'int sqlite3BtreeBeginStmt(Btree *p, int iStatement) {\n  if (p->flags & BTREE_SINGLE) return SQLITE_OK;')
    b = replace(b, '  p->db = db;', '  p->db = db;\n  p->creatorPid = getpid();', count=2)
    b = replace(b, 'int sqlite3BtreeBeginTrans(Btree *p, int wrFlag, int *pSchemaVersion) {',
        'int sqlite3BtreeBeginTrans(Btree *p, int wrFlag, int *pSchemaVersion) {\n'
        '  if (p->creatorPid != getpid()) return SQLITE_IOERR;')
    b = replace(b, 'int sqlite3BtreeCreateTable(Btree *p, Pgno *piTable, int flags) {',
        'int sqlite3BtreeCreateTable(Btree *p, Pgno *piTable, int flags) {\n'
        '  if (p->flags & BTREE_SINGLE) { *piTable = 1; return SQLITE_OK; }')
    b = replace(b, '  if (p == NULL) return SQLITE_OK;\n\n  /* Walk the savepoint stack',
        '  if (p == NULL || (p->flags & BTREE_SINGLE)) return SQLITE_OK;\n'
        '  if (iSavepoint < 0) {\n'
        '    if (p->inTrans != SQLITE_TXN_WRITE) return SQLITE_OK;\n'
        '    sqlite3ErrorWithMsg(p->db, SQLITE_ERROR, "ULMDB B-tree prototype: root SAVEPOINT rollback is unavailable");\n'
        '    return SQLITE_ERROR;\n  }\n\n  /* Walk the savepoint stack')
    # Unlike LMDB pages, ULMDB write-returned slices may be retired by the
    # next mutation.  Never retain such a slice in a sibling cursor/VM row.
    b = replace(b, '  cursorCacheClear(pCur);\n}\n\n/* Return a pointer to a copy',
        '  if (pCur->pBtree) {\n'
        '    BtCursor *sibling;\n'
        '    for (sibling = pCur->pBtree->first_cursor; sibling; sibling = sibling->next)\n'
        '      cursorCacheClear(sibling);\n'
        '  } else cursorCacheClear(pCur);\n}\n\n/* Return a pointer to a copy')
    start = b.index('/* Rename the finished VACUUM copy')
    end = b.index('/* sqlite3BtreeIncrVacuum:', start)
    b = b[:start] + '''int sqlite3BtreeCopyFile(Btree *dst, Btree *src) {
  (void)src;
  sqlite3ErrorWithMsg(dst->db, SQLITE_ERROR, "ULMDB B-tree prototype: VACUUM is unavailable");
  return SQLITE_ERROR;
}

''' + b[end:]
    start = b.index('const void *sqlite3BtreePayloadFetch(')
    end = b.index('\n/*', start)
    b = b[:start] + '''const void *sqlite3BtreePayloadFetch(BtCursor *pCur, u32 *pAmt){
  const unsigned char *row;
  MDB_val key, data;
  int rc;
  if (pCur->pBtree->pEph) {
    row = ephPayload(pCur, pAmt);
  } else {
    if (pCur->curValid) {
      data = pCur->curData;
    } else {
      rc = restoreCursor(pCur, 1);
      if (rc != SQLITE_OK) { *pAmt = 0; return "\\1"; }
      rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
      if (rc) { *pAmt = 0; return "\\1"; }
      cursorCacheSet(pCur, &key, &data);
    }
    row = (const unsigned char *)data.mv_data;
    *pAmt = (u32)data.mv_size;
  }
  if (!row) { *pAmt = 0; return "\\1"; }
  if (pCur->pBtree->inTrans == SQLITE_TXN_WRITE || pCur->pBtree->pEph) {
    if (*pAmt > pCur->stableRowCap) {
      unsigned char *copy = sqlite3Realloc(pCur->stableRow, *pAmt);
      if (!copy) {
        sqlite3OomFault(pCur->pBtree->db);
        *pAmt = 0;
        /* A nonzero header byte forces Column's checked Payload slow path. */
        return "\\1";
      }
      pCur->stableRow = copy; pCur->stableRowCap = *pAmt;
    }
    if (*pAmt) memcpy(pCur->stableRow, row, *pAmt);
    row = pCur->stableRow;
  }
  /* Preserve SQLite's local-page payload ceiling, including debug builds. */
  if (*pAmt > 65536) *pAmt = 65536;
  return row ? row : (const void *)"";
}
''' + b[end:]
    # SQLite serializes a connection, not different connections.  Protect the
    # process-wide writer list so separate connections cannot race its links.
    start = b.index('static int lumoProcessWriteLock(')
    end = b.index('\nstruct BtCursor {', start)
    b = b[:start] + '''static int lumoProcessWriteLock(Btree *p) {
  Btree *q;
  int rc = SQLITE_OK;
  sqlite3_mutex *mutex = sqlite3_mutex_alloc(SQLITE_MUTEX_STATIC_APP1);
  sqlite3_mutex_enter(mutex);
  if (!p->hasProcessWriteLock) {
    for (q = lumoProcessWriters; q; q = q->nextProcessWriter) {
      if (q != p && strcmp(q->path, p->path) == 0) { rc = SQLITE_BUSY; break; }
    }
    if (rc == SQLITE_OK) {
      p->nextProcessWriter = lumoProcessWriters;
      lumoProcessWriters = p;
      p->hasProcessWriteLock = 1;
    }
  }
  sqlite3_mutex_leave(mutex);
  return rc;
}
static void lumoProcessWriteUnlock(Btree *p) {
  Btree **pp;
  sqlite3_mutex *mutex = sqlite3_mutex_alloc(SQLITE_MUTEX_STATIC_APP1);
  sqlite3_mutex_enter(mutex);
  if (p->hasProcessWriteLock) {
    for (pp = &lumoProcessWriters; *pp; pp = &(*pp)->nextProcessWriter) {
      if (*pp == p) { *pp = p->nextProcessWriter; break; }
    }
    p->nextProcessWriter = NULL; p->hasProcessWriteLock = 0;
  }
  sqlite3_mutex_leave(mutex);
}
''' + b[end:]
    # Private temporary environments use an owner-only directory.  No unlink
    # of an arbitrary user namespace; only these mkdtemp paths reach cleanup.
    start = b.index('static void removeTempDb(')
    end = b.index('\n#ifndef SQLITE_MUTEX_STATIC_TEMPDIR', start)
    b = b[:start] + '''static int removeTempEntry(const char *path, const struct stat *st,
                           int type, struct FTW *walk) {
  (void)st; (void)type; (void)walk;
  return remove(path);
}
static void removeTempDb(const char *path) {
  if (path && *path) (void)nftw(path, removeTempEntry, 16, FTW_DEPTH | FTW_PHYS);
}
''' + b[end:]
    b = replace(b, '    int tmpfd;\n    mdbFlags |= MDB_NOSYNC | MDB_NOSUBDIR;',
                '    mdbFlags |= MDB_NOSYNC;')
    b = replace(b, '    tmpfd = mkstemp(dirPathName);\n    if (tmpfd < 0) {',
                '    if (mkdtemp(dirPathName) == NULL) {')
    b = replace(b, '    close(tmpfd);\n', '')
    # One persistent store per SQLite connection: phase-one native publication
    # cannot implement SQLite's multi-file super-journal contract.
    b = replace(b, '  mdbFlags = MDB_NOTLS;', '''  if (!isTempDb && !isMemdb && db->aDb[0].pBt != NULL) {
    sqlite3ErrorWithMsg(db, SQLITE_ERROR, "ULMDB B-tree prototype: ATTACH is unavailable");
    *ppBtree = NULL;
    return SQLITE_ERROR;
  }
  mdbFlags = MDB_NOTLS;''')
    b = replace(b, '  int envClose = 0;\n  int mdbFlags;\n',
                '  int envClose = 0;\n  int mdbFlags;\n  int existingStore = 0;\n')
    b = replace(b, '    sqlite3OsFullPathname(pVfs, zFilename, sizeof(dirPathName), dirPathName);',
        '    rc = sqlite3OsFullPathname(pVfs, zFilename, sizeof(dirPathName), dirPathName);\n'
        '    if (rc != SQLITE_OK) { *ppBtree = NULL; return rc; }\n'
        '    if (!(vfsFlags & SQLITE_OPEN_CREATE)) {\n'
        '      struct stat existing;\n'
        '      if (lstat(dirPathName, &existing) != 0) { *ppBtree = NULL; return SQLITE_CANTOPEN; }\n'
        '    }')
    b = replace(b, '  rc = mdb_env_open(p->env, dirPathName, mdbFlags, SQLITE_DEFAULT_FILE_PERMISSIONS);', '''  if (!p->isTemp) {
    struct stat st;
    char nativePath[BTREE_MAX_PATH + 8];
    sqlite3_snprintf(sizeof(nativePath), nativePath, "%s.ulmdb", dirPathName);
    existingStore = lstat(nativePath, &st) == 0;
    if (lstat(dirPathName, &st) == 0 && st.st_size != 0) {
      char marker[19];
      int fd = open(dirPathName, O_RDONLY | O_NOFOLLOW);
      ssize_t n = fd < 0 ? -1 : read(fd, marker, sizeof(marker));
      if (fd >= 0) close(fd);
      if (n != sizeof(marker) || memcmp(marker, "ULMDB-LMDB-ADAPTER\\n", sizeof(marker))) {
        rc = SQLITE_NOTADB; goto error_rc;
      }
    }
  }
  rc = mdb_env_open(p->env, dirPathName, mdbFlags, 0600);''')
    # Native FULL durability is the only persistent profile.  No sync knob.
    b = replace(b, '    const char *_lv = "normal";                 /* LumoSQL default floor */',
                '    const char *_lv = "full";                   /* ULMDB durable publication */')
    # A distinct format marker rejects another ULMDB/LMDB application's store.
    b = replace(b, '  if (rc == 0) {\n    p->transType = get_meta_32_direct(txn, dbi, LUMO_LMDB_TRANSACTION);', '''  if (rc == 0) {
    unsigned char tag = 200;
    MDB_val fk = { 1, &tag }, fv;
    rc = mdb_get(txn, dbi, &fk, &fv);
    if (rc || fv.mv_size != 16 || memcmp(fv.mv_data, "ULMDB-SQL-BT-01\\0", 16)) {
      rc = SQLITE_NOTADB; goto error_rc;
    }
    p->transType = get_meta_32_direct(txn, dbi, LUMO_LMDB_TRANSACTION);''')
    b = replace(b, '  if (isReadonly) goto error;\n',
                '  if (isReadonly) goto error;\n  if (existingStore) { rc = SQLITE_NOTADB; goto error_rc; }\n')
    b = replace(b, '  p->transType = get_meta_32_direct(txn, dbi, LUMO_LMDB_TRANSACTION);\n  if (p->transType', '''  {
    unsigned char tag = 200;
    MDB_val fk = { 1, &tag }, fv = { 16, "ULMDB-SQL-BT-01\\0" };
    rc = mdb_put(txn, dbi, &fk, &fv, 0);
    if (rc) goto error;
  }
  p->transType = get_meta_32_direct(txn, dbi, LUMO_LMDB_TRANSACTION);
  if (p->transType''')
    # Snapshot promotion compares the actual immutable checkpoint, including
    # branch/restore identity, before exposing a new write transaction.
    b = replace(b, '      u64 uCounterRO = get_meta_64(p, LUMO_LMDB_META_COUNTER), uCounterRW;', '''      unsigned char baseRead[64], baseWrite[64];
      rc = ulmdb_txn_base_checkpoint(p->svp->txn, baseRead);
      if (rc) { lumoProcessWriteUnlock(p); return error_map(rc); }''')
    start = b.index('      uCounterRW = get_meta_64(')
    end = b.index('      p->inTrans = SQLITE_TXN_WRITE;', start)
    b = b[:start] + '''      rc = ulmdb_txn_base_checkpoint(p->svp->txn, baseWrite);
      if (rc || memcmp(baseRead, baseWrite, sizeof(baseRead))) {
        mdb_txn_abort(p->svp->txn);
        sqlite3_free(p->svp); p->svp = NULL;
        p->hasData = 0; p->inTrans = SQLITE_TXN_NONE;
        lumoProcessWriteUnlock(p);
        return rc ? error_map(rc) : SQLITE_BUSY_SNAPSHOT;
      }
''' + b[end:]
    # 3.54 adds this debug/cursor-hint entry point.  Hints are optional.
    b += '\n#if defined(SQLITE_ENABLE_CURSOR_HINTS) && defined(SQLITE_DEBUG)\nBtCursor *sqlite3BtreeCursorHintTblCsr(BtCursor *p) { (void)p; return NULL; }\n#endif\n'
    # A rolled-back child can invalidate DBI handles even when malloc reuses
    # the same stack-node address for the next child.
    b = re.sub(r'^(\s*)p->svp = ([^;\n]+);',
               r'\1p->gtCacheValid = 0;\n\1p->svp = \2;', b, flags=re.MULTILINE)
    b = replace(b, '    if (_lv && *_lv) lumo_apply_sync(p, _lv);',
                '    if (_lv && *_lv) {\n      rc = lumo_apply_sync(p, _lv);\n      if (rc) goto error_rc;\n    }')
    # Open the native cursor lazily for immutable rowid readers.  Exact hits
    # use mdb_get; a range miss or relative move materializes and seeks the
    # logical current key before using NEXT/PREV.
    b = replace(b, '  rc = mdb_cursor_open(pCur->txn, pCur->dbi, &pCur->cursor);',
        '  pCur->lazyRead = !wrFlag && !pKeyInfo && p->inTrans == SQLITE_TXN_READ\n'
        '                   && p->transType != LMDB_TRANSACTION_SERIALISE;\n'
        '  rc = pCur->lazyRead ? 0 : mdb_cursor_open(pCur->txn, pCur->dbi, &pCur->cursor);')
    b = replace(b, '  /* SQLite positions a new cursor before reading from it */',
        '  rc = ulmdbAttachReader(pCur);\n'
        '  if (rc != SQLITE_OK) {\n'
        '    if (pCur->cursor) mdb_cursor_close(pCur->cursor);\n'
        '    pCur->cursor = NULL;\n    return rc;\n  }\n'
        '  /* SQLite positions a new cursor before reading from it */')
    b = replace(b, '  if (pCur->ephCur) ephCursorClose(pCur);\n  p = pCur->pBtree;',
        '  if (pCur->ephCur) ephCursorClose(pCur);\n'
        '  ulmdbReleaseReader(pCur);\n  p = pCur->pBtree;')
    # Promotion retains the old actual reader through its cursor holds. The
    # new writer is admitted while that immutable base is still pinned.
    b = replace(b, '      mdb_txn_abort(p->svp->txn);\n      /* Aborting the read transaction closed',
        '      MDB_txn *oldReader = p->svp->txn;\n      /* Ending the read transaction closes')
    b = replace(b, '      rc = mdb_txn_begin(p->env, NULL, 0, &p->svp->txn);\n      if (rc) goto out_error;',
        '      rc = mdb_txn_begin(p->env, NULL, 0, &p->svp->txn);\n'
        '      ulmdbRetireReader(p, oldReader);\n      if (rc) goto out_error;')
    b = replace(b, '      mdb_txn_abort(p->svp->txn);\n      LUMO_LOG("sqlite3BtreeCommitPhaseOne: abort r/o',
        '      if (ulmdbRealReader(p)) ulmdbRetireReader(p, p->svp->txn);\n'
        '      else mdb_txn_abort(p->svp->txn);\n'
        '      LUMO_LOG("sqlite3BtreeCommitPhaseOne: abort r/o')
    b = replace(b, '        mdb_txn_reset(p->svp->txn);',
        '        ulmdbParkReader(p);\n        mdb_txn_reset(p->svp->txn);')
    b = replace(b, '    invalidateCursors(p, p->svp, tripCode, writeOnly);\n    mdb_txn_abort(p->svp->txn);',
        '    invalidateCursors(p, p->svp, tripCode, writeOnly);\n'
        '    if (ulmdbRealReader(p)) ulmdbRetireReader(p, p->svp->txn);\n'
        '    else mdb_txn_abort(p->svp->txn);')
    b = replace(b, '    if (p->svp->txn) mdb_txn_abort(p->svp->txn);\n    sqlite3_free(p->svp);',
        '    if (p->svp->txn) {\n'
        '      if (ulmdbRealReader(p)) ulmdbRetireReader(p, p->svp->txn);\n'
        '      else mdb_txn_abort(p->svp->txn);\n    }\n'
        '    sqlite3_free(p->svp);')
    insert = b.index('/*\n** Move the cursor to the entry with rowid intKey')
    b = b[:insert] + (HERE / 'read_paths.c').read_text() + '\n' + b[insert:]
    b = replace(b, '  rc = mdb_cursor_get(pCur->cursor, key, &data, MDB_SET_RANGE);', '''  if (!pCur->cursor && pCur->lazyRead && pCur->pBtree->inTrans == SQLITE_TXN_READ) {
    rc = mdb_get(pCur->pBtree->svp->txn, pCur->dbi, key, &data);
    if (rc == 0) {
      memcpy(pCur->logicalKey, key[0].mv_data, 8);
      key[0].mv_data = pCur->logicalKey;
      cursorCacheSet(pCur, &key[0], &data);
      *pRes = 0;
      return SQLITE_OK;
    }
    if (rc != MDB_NOTFOUND) return error_map(rc);
  }
  rc = ulmdbMaterializeReadCursor(pCur, 0);
  if (rc != SQLITE_OK) return rc;
  rc = mdb_cursor_get(pCur->cursor, key, &data, MDB_SET_RANGE);''')
    b = replace(b, 'static int cursorMove(BtCursor *pCur, int *pRes, MDB_cursor_op op, int reposition) {\n  MDB_val key, data;\n  int rc;',
        'static int cursorMove(BtCursor *pCur, int *pRes, MDB_cursor_op op, int reposition) {\n'
        '  MDB_val key, data;\n  int rc;\n'
        '  if (!pCur->cursor && (!pCur->cleared || (op != MDB_NEXT && op != MDB_PREV))) {\n'
        '    rc = ulmdbMaterializeReadCursor(pCur, reposition);\n'
        '    if (rc != SQLITE_OK) return rc;\n  }')
    b = replace(b, '          && pCur->cursor != NULL\n          && pCur->tripCode == SQLITE_OK',
        '          && (pCur->cursor != NULL || (pCur->lazyRead && pCur->curValid))\n'
        '          && pCur->tripCode == SQLITE_OK')
    b = replace(b, '    /* Whatever happens below (position saved + cursor closed, or cursor', '''    /* A lazy exact hit has a real logical position but no native cursor.
    ** Save only its owned key before dropping this snapshot capability. */
    if (pCur->lazyRead && !pCur->cursor) {
      if (pCur->curValid && !pCur->cleared && !pCur->atEof
          && (tripCode == SQLITE_OK || !(pCur->wrFlag || !writeOnly))) {
        void *saved = sqlite3DbMallocRaw(0, 8);
        if (!saved) { pCur->tripCode = SQLITE_NOMEM; if (!rc) rc = ENOMEM; }
        else {
          memcpy(saved, pCur->logicalKey, 8);
          sqlite3DbFree(0, pCur->savedKey.mv_data);
          pCur->savedKey.mv_data = saved; pCur->savedKey.mv_size = 8;
        }
      }
      if (pCur->tripCode == SQLITE_OK && (pCur->wrFlag || !writeOnly))
        pCur->tripCode = tripCode;
    }
    pCur->lazyRead = 0;
    /* Whatever happens below (position saved + cursor closed, or cursor''')
    return native_flavor(writer_cache(direct_put(writer_carrier(b)))) + '\n' + (HERE / 'lumosql/ephemeral.c').read_text()


def writer_cache(b):
    # SQLite closes a statement transaction using its zero-based savepoint
    # label. A missing label would also merge a surrounding named savepoint.
    b = replace(b, '  S->nSavepoint = iStatement;',
        '  S->nSavepoint = iStatement - 1;')
    b = replace(b, '  if (p->creatorPid != getpid()) return SQLITE_IOERR;',
        '#ifdef ULMDB_BTREE_NATIVE\n'
        '  if ((unsigned)p->creatorPid != ubt_process_id_fast()) return SQLITE_IOERR;\n'
        '#else\n  if (p->creatorPid != getpid()) return SQLITE_IOERR;\n#endif')
    b = replace(b, '  unsigned char writerBase[64];\n',
        '  unsigned char writerBase[64];\n#ifdef ULMDB_BTREE_NATIVE\n'
        '  u64 writerCounter;\n  u32 writerSchema;\n  u8 writerMetaValid;\n#endif\n')
    insert = b.index('/* get an integer from the "dataDbi" table')
    b = b[:insert] + (HERE / 'writer_cache.c').read_text() + '\n' + b[insert:]
    b = replace(b, '  return get_meta_32_direct(p->svp->txn, p->dataDbi, idx);', '''#ifdef ULMDB_BTREE_NATIVE
  if (idx == BTREE_SCHEMA_VERSION && ulmdbWriterCacheActive(p)) {
    u64 value;
    int rc = ubt_txn_validate(p->svp->txn);
    if (!rc) rc = ulmdbWriterMeta(p, idx, &value);
    return rc ? 0 : (u32)value;
  }
#endif
  return get_meta_32_direct(p->svp->txn, p->dataDbi, idx);''')
    b = replace(b, '  key.mv_data = &bidx;\n  key.mv_size = 1;\n  rc = mdb_get(p->svp->txn, p->dataDbi, &key, &data);', '''#ifdef ULMDB_BTREE_NATIVE
  if (idx == LUMO_LMDB_META_COUNTER && ulmdbWriterCacheActive(p)) {
    u64 value;
    int rc = ubt_txn_validate(p->svp->txn);
    if (!rc) rc = ulmdbWriterMeta(p, idx, &value);
    return rc ? 0 : value;
  }
#endif
  key.mv_data = &bidx;
  key.mv_size = 1;
  rc = mdb_get(p->svp->txn, p->dataDbi, &key, &data);''')
    b = replace(b, '  return put_meta_32_direct(p->svp->txn, p->dataDbi, idx, iValue);', '''  {
    int rc = put_meta_32_direct(p->svp->txn, p->dataDbi, idx, iValue);
#ifdef ULMDB_BTREE_NATIVE
    ulmdbWriterMetaWritten(p, idx, 4, iValue, rc);
#endif
    return rc;
  }''')
    b = replace(b, '  rc = put_meta_32_direct(use_txn, dbi, idx, iValue);\n  if (rc) goto error;', '''  rc = put_meta_32_direct(use_txn, dbi, idx, iValue);
#ifdef ULMDB_BTREE_NATIVE
  if (p->svp && use_txn == p->svp->txn)
    ulmdbWriterMetaWritten(p, idx, 4, iValue, rc);
#endif
  if (rc) goto error;''')
    b = replace(b, '  return mdb_put(p->svp->txn, p->dataDbi, &key, &data, 0);', '''  {
    int rc = mdb_put(p->svp->txn, p->dataDbi, &key, &data, 0);
#ifdef ULMDB_BTREE_NATIVE
    ulmdbWriterMetaWritten(p, idx, 8, uValue, rc);
#endif
    return rc;
  }''')
    b = replace(b, '  {\n    u64 counter = get_meta_64(p, LUMO_LMDB_META_COUNTER);', '''#ifdef ULMDB_BTREE_NATIVE
  if (ulmdbWriterCacheActive(p))
    return error_map(ulmdbWriterVersion(p, pSchemaVersion));
#endif
  {
    u64 counter = get_meta_64(p, LUMO_LMDB_META_COUNTER);''')
    b = replace(b, '      u64 uCounter = get_meta_64(p, LUMO_LMDB_META_COUNTER) + 1;\n      rc = put_meta_64(p, LUMO_LMDB_META_COUNTER, uCounter);', '''      u64 uCounter;
#ifdef ULMDB_BTREE_NATIVE
      rc = ubt_txn_validate(p->svp->txn);
      if (!rc) rc = ulmdbWriterMeta(p, LUMO_LMDB_META_COUNTER, &uCounter);
      if (!rc) { uCounter++; rc = put_meta_64(p, LUMO_LMDB_META_COUNTER, uCounter); }
#else
      uCounter = get_meta_64(p, LUMO_LMDB_META_COUNTER) + 1;
      rc = put_meta_64(p, LUMO_LMDB_META_COUNTER, uCounter);
#endif''')
    for signature in ['static int ulmdbBeginSqlWriter(Btree *p, MDB_txn **out) {',
                      'static void ulmdbAbortSqlTransaction(Btree *p, MDB_txn *txn) {',
                      'static int ulmdbCommitSqlWriter(Btree *p) {',
                      'static int ulmdbRollbackRootSavepoint(Btree *p) {',
                      'int sqlite3BtreeClose(Btree *p) {']:
        b = replace(b, signature, signature + '\n  ulmdbWriterCacheClear(p);')
    b = replace(b, '      rc1 = mdb_txn_commit(S->txn);',
        '      rc1 = mdb_txn_commit(S->txn);\n      if (rc1) ulmdbWriterCacheClear(p);')
    b = replace(b, '      rc = mdb_txn_commit(S->txn);',
        '      rc = mdb_txn_commit(S->txn);\n      if (rc) ulmdbWriterCacheClear(p);')
    b = replace(b, 'mdb_txn_abort(S->txn);',
        'ulmdbWriterCacheClear(p);\n    mdb_txn_abort(S->txn);', count=3)
    return b


def writer_carrier(b):
    b = replace(b, '  MDB_env *env;\n',
        '  MDB_env *env;\n  MDB_txn *outerWriter;\n  unsigned char writerBase[64];\n')
    insert = b.index('int sqlite3BtreeOpen(')
    b = b[:insert] + (HERE / 'writer_carrier.c').read_text() + '\n' + b[insert:]
    # A child suspends its parent's C capabilities. Save/close every current
    # cursor before admission, retaining the SQLite header and owned result.
    for label in ['N', 'S']:
        anchor = f'  rc = mdb_txn_begin(p->env, p->svp ? p->svp->txn : NULL, 0, &{label}->txn);'
        indent = '    ' if label == 'N' else '  '
        # The named-savepoint loop has two additional spaces in its source.
        actual = indent + anchor.lstrip()
        replacement = (indent + 'rc = invalidateCursors(p, p->svp, SQLITE_OK, 0);\n'
                       + indent + f'if (rc) {{ sqlite3_free({label}); return error_map(rc); }}\n'
                       + actual)
        b = replace(b, actual, replacement)
    # A saved/lazy header still needs its requested error even after a prior
    # invalidation closed the native cursor capability.
    b = replace(b, '    if (pCur->cursor != NULL) {\n      mdb_cursor_close(pCur->cursor);\n      pCur->cursor = NULL;\n    }\n  }\n  return rc;',
        '    if (pCur->cursor != NULL) {\n      mdb_cursor_close(pCur->cursor);\n      pCur->cursor = NULL;\n    }\n'
        '    if (pCur->tripCode == SQLITE_OK && (pCur->wrFlag || !writeOnly))\n'
        '      pCur->tripCode = tripCode;\n  }\n  return rc;')
    b = replace(b, '  rc = invalidateCursors(p, S->prev, SQLITE_ABORT_ROLLBACK, 0);',
        '  rc = invalidateCursors(p, S->prev, SQLITE_ABORT_ROLLBACK,\n'
        '                         (p->db->mDbFlags & DBFLAG_SchemaChange) == 0);')
    b = replace(b, 'mdb_txn_begin(p->env, NULL, 0, &p->svp->txn)',
                'ulmdbBeginSqlWriter(p, &p->svp->txn)')
    b = replace(b, 'mdb_txn_begin(p->env, NULL, wrFlag ? 0 : MDB_RDONLY, &p->svp->txn)',
        '(wrFlag ? ulmdbBeginSqlWriter(p, &p->svp->txn)\n'
        '                   : mdb_txn_begin(p->env, NULL, MDB_RDONLY, &p->svp->txn))')
    b = replace(b, 'mdb_txn_begin(p->env, NULL, isReadonly ? MDB_RDONLY : 0, &p->svp->txn)',
        '(isReadonly ? mdb_txn_begin(p->env, NULL, MDB_RDONLY, &p->svp->txn)\n'
        '                       : ulmdbBeginSqlWriter(p, &p->svp->txn))')
    b = replace(b, '      rc = ulmdb_txn_base_checkpoint(p->svp->txn, baseWrite);',
        '      memcpy(baseWrite, p->writerBase, sizeof(baseWrite));\n      rc = 0;')
    # Ordinary aborts end the complete writer tree. Only the root-savepoint
    # helper above deliberately aborts/recreates the SQL child in isolation.
    b = b.replace('mdb_txn_abort(p->svp->txn);',
                  'ulmdbAbortSqlTransaction(p, p->svp->txn);')
    b = replace(b, '      rc = mdb_txn_commit(p->svp->txn);',
                  '      rc = ulmdbCommitSqlWriter(p);')
    start = b.index('  if (iSavepoint < 0) {')
    end = b.index('\n\n  /* Walk the savepoint stack', start)
    b = b[:start] + '  if (iSavepoint < 0) return ulmdbRollbackRootSavepoint(p);' + b[end:]
    # SERIALISE mode also uses a native writer while SQL labels it READ.
    b = replace(b, '  if (pCur->pBtree->inTrans == SQLITE_TXN_WRITE || pCur->pBtree->pEph) {',
        '  if (pCur->pBtree->outerWriter || pCur->pBtree->pEph) {')
    return b


def direct_put(b):
    b = replace(b, '  u8 lazyRead;\n', '  u8 lazyRead;\n  u8 writePositionDirty;\n')
    b = replace(b, '  pCur->curKey = *key;',
                '  pCur->writePositionDirty = 0;\n  pCur->curKey = *key;')
    # Save the new logical position before suspending/aborting its native
    # transaction. A pre-write native cursor may still point at a different row.
    b = replace(b, '    /* A lazy exact hit has a real logical position', '''    if (pCur->writePositionDirty) {
      if (pCur->tripCode == SQLITE_OK
          && (tripCode == SQLITE_OK || !(pCur->wrFlag || !writeOnly))) {
        void *saved = sqlite3DbMallocRaw(0, 8);
        if (!saved) { pCur->tripCode = SQLITE_NOMEM; if (!rc) rc = ENOMEM; }
        else {
          memcpy(saved, pCur->logicalKey, 8);
          sqlite3DbFree(0, pCur->savedKey.mv_data);
          pCur->savedKey.mv_data = saved; pCur->savedKey.mv_size = 8;
        }
      }
      if (pCur->cursor) mdb_cursor_close(pCur->cursor);
      pCur->cursor = NULL;
      pCur->writePositionDirty = 0;
    }
    /* A lazy exact hit has a real logical position''')
    b = replace(b, '  /* if the cursor is valid, that\'ll be all */', '''  if (pCur->writePositionDirty) {
    if (!reposition) return SQLITE_OK;
    void *saved = sqlite3DbMallocRaw(0, 8);
    if (!saved) return SQLITE_NOMEM;
    memcpy(saved, pCur->logicalKey, 8);
    sqlite3DbFree(0, pCur->savedKey.mv_data);
    pCur->savedKey.mv_data = saved; pCur->savedKey.mv_size = 8;
    pCur->writePositionDirty = 0;
  }
  /* if the cursor is valid, that'll be all */''')
    b = replace(b, '  if (pCur->tripCode != SQLITE_OK || pCur->savedKey.mv_data) {',
        '  if (pCur->tripCode != SQLITE_OK || pCur->savedKey.mv_data || pCur->writePositionDirty) {')
    b = replace(b, '  /* curValid means the row was cached by the last positioning operation',
        '  if (pCur->writePositionDirty) return 1;\n'
        '  /* curValid means the row was cached by the last positioning operation')
    b = replace(b, '  pCur->cleared = 1;\n}\n\n/* Upstream writes 1 or 2',
        '  pCur->cleared = 1;\n  pCur->writePositionDirty = 0;\n}\n\n/* Upstream writes 1 or 2')
    start = b.index('i64 sqlite3BtreeIntegerKey(')
    end = b.index('\nvoid sqlite3BtreeCursorPin(', start)
    section = b[start:end]
    section = replace(section, '  } else {\n    rc = mdb_cursor_get',
        '  } else {\n    rc = restoreCursor(pCur, 1);\n    if (rc != SQLITE_OK) return 0;\n    rc = mdb_cursor_get')
    b = b[:start] + section + b[end:]
    b = replace(b, '  if (pCur == NULL || pCur->cursor == NULL) return 0;\n  rc = mdb_cursor_get',
        '  if (pCur == NULL) return 0;\n'
        '  rc = restoreCursor(pCur, 1);\n'
        '  if (rc != SQLITE_OK || pCur->cursor == NULL) return 0;\n  rc = mdb_cursor_get')
    # Incremental reads may begin with a lazy exact-read header as well.
    start = b.index('int sqlite3BtreePayloadChecked(')
    end = b.index('\n/* sqlite3BtreePutData,', start)
    section = b[start:end]
    section = replace(section, '  if (pCur->cursor == NULL && !pCur->savedKey.mv_data) {',
        '  if (pCur->lazyRead && !pCur->cursor && pCur->curValid) {\n'
        '    rc = ulmdbMaterializeReadCursor(pCur, 1);\n'
        '    if (rc != SQLITE_OK) return rc;\n  }\n'
        '  if (pCur->cursor == NULL && !pCur->savedKey.mv_data) {')
    b = b[:start] + section + b[end:]
    b = replace(b, 'insert_it:\n', '''insert_it:
  if (!pCur->pBtree->isTemp && !pCur->pKeyInfo && !pX->nZero
      && !(flags & BTREE_PREFORMAT)) {
    /* SQLite3.54.1 passes BTREE_APPEND only as TableMoveto's biasRight;
    ** it is a search hint. Ordinary put preserves all insertion semantics. */
    assert(mdbFlags == 0);
    rc = mdb_put(pCur->pBtree->svp->txn, pCur->dbi, key, &data, 0);
    if (rc) goto out_map;
    cursorCacheClearAfterWrite(pCur);
    /* Only this owned key survives. Never retain SQLite's input row bytes. */
    memcpy(pCur->logicalKey, bKey, 8);
    pCur->curKey.mv_data = pCur->curData.mv_data = NULL;
    pCur->curKey.mv_size = pCur->curData.mv_size = 0;
    pCur->writePositionDirty = 1;
    pCur->atEof = pCur->cleared = 0;
    return SQLITE_OK;
  }
''')
    b = replace(b, '  pCur->cleared = 0;        /* the LMDB cursor is on the inserted row */',
        '  pCur->writePositionDirty = 0;\n'
        '  pCur->cleared = 0;        /* the LMDB cursor is on the inserted row */')
    return b


def native_flavor(b):
    # Keep native row accessors inside the VM; the SDK flavor keeps its definitions.
    for signature in (
        'i64 sqlite3BtreeIntegerKey(BtCursor *pCur) {',
        'const void *sqlite3BtreePayloadFetch(BtCursor *pCur, u32 *pAmt){',
        'u32 sqlite3BtreePayloadSize(BtCursor *pCur){',
    ):
        b = replace(b, signature,
                    '#ifdef ULMDB_BTREE_NATIVE\nSQLITE_INLINE\n#endif\n' + signature)
    start = b.index('  /* Map size: SSIZE_MAX/8')
    end = b.index('  mdb_env_set_maxreaders(', start)
    b = b[:start] + '#ifndef ULMDB_BTREE_NATIVE\n' + b[start:end] + '#endif\n' + b[end:]
    start = b.index('  if ((mdbFlags & MDB_RDONLY) && !isTempDb && !isMemdb) {')
    end = b.index('  if (!p->isTemp) {\n    struct stat st;\n    char nativePath', start)
    b = b[:start] + '#ifndef ULMDB_BTREE_NATIVE\n' + b[start:end] + '#endif\n' + b[end:]
    start = b.index('  if (!p->isTemp) {\n    struct stat st;\n    char nativePath')
    end = b.index('  rc = mdb_env_open(p->env, dirPathName, mdbFlags, 0600);', start)
    b = b[:start] + '#ifndef ULMDB_BTREE_NATIVE\n' + b[start:end] + '''#else
  /* The private profile validates a dedicated directory and its own magic.
  ** Its publication policy is FULL for every native store, including temp. */
  mdbFlags &= ~(MDB_NOSUBDIR | MDB_NOSYNC | MDB_NOMETASYNC);
  p->mdbFlags = mdbFlags;
#endif
''' + b[end:]
    return b


def generate(source):
    raw = source.read_bytes()
    if hashlib.sha256(raw).hexdigest() != SQLITE_SHA256:
        raise ValueError('expected the verified official SQLite 3.54.1 amalgamation')
    upstream = json.loads((HERE / 'lumosql/provenance.json').read_text())
    for name, item in upstream['files'].items():
        if hashlib.sha256((HERE / 'lumosql' / name).read_bytes()).hexdigest() != item['sha256']:
            raise ValueError(f'vendored source hash changed: {name}')
    text = raw.decode()
    pager = (HERE / 'lumosql/pager.c').read_text()
    # Report the actual absence of a SQLite journal/WAL; never echo WAL.
    pager = replace(pager, '  cached_journal_mode = eMode;\n  return eMode;',
                    '  (void)eMode;\n  return PAGER_JOURNALMODE_OFF;')
    pager += '\nvoid sqlite3PagerWalStat(Pager *p, sqlite3_str *s) { (void)p; (void)s; }\n'
    start = text.index('/************** Begin file pager.c ')
    end = text.index('/************** End of backup.c ', start)
    end = text.index('\n', end) + 1
    return text[:start] + pager + '\n' + backend() + '\n' + (HERE / 'unsupported.c').read_text() + '\n' + text[end:]


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('sqlite3', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    output = generate(args.sqlite3)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(output)
