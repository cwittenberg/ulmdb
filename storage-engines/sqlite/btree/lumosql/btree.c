/*

  Copyright 2021 The LumoSQL Authors under the terms contained in the file LICENSES/MIT

  SPDX-License-Identifier: MIT
  SPDX-FileCopyrightText: 2021 The LumoSQL Authors
  SPDX-ArtifactOfProjectName: LumoSQL
  SPDX-FileType: Code
  SPDX-FileComment: Original by Claudio Calvelli, 2021

  not-fork.d/lmdb/files/btree.c
*/

// This version omits shared cache code.

/* This version does not lock its own structures, including the
** process-wide writer list, against concurrent use from several threads.
** LMDB requires a write transaction and its cursors to be used only by the
** thread that began it, so a connection must stay on one thread while it
** has a write transaction open.  Every env is opened with MDB_NOTLS, so read
** transactions are not tied to a thread. */

#ifndef __LUMO_BACKEND_btree_c
#define __LUMO_BACKEND_btree_c 1

#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <limits.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include "lmdb.h"

/* Pre-3.34 SQLite didn't expose the SQLITE_TXN_* enum: those names and
** the public sqlite3_txn_state() API came in with the December-2020
** release.  The LMDB shim only uses these as internal state-machine
** sentinels for its own Btree::inTrans field, so any stable distinct
** values are fine on older SQLite. */
#if SQLITE_VERSION_NUMBER < 3034000
# ifndef SQLITE_TXN_NONE
#  define SQLITE_TXN_NONE  0
#  define SQLITE_TXN_READ  1
#  define SQLITE_TXN_WRITE 2
# endif
#endif

/* maximum length of a filename we consider opening */
#define BTREE_MAX_PATH 1024

/* LMDB caps stored keys: 511 bytes in LMDB 0.9, and a limit derived from
** the page size in LMDB 1.0.  A WITHOUT ROWID table, or a UNIQUE index
** whose columns are all NOT NULL, stores its ordering fields in the key and
** spills the full record into the value.  Other indexes store the whole
** record as the key.  An insert whose stored key exceeds the cap returns
** SQLITE_TOOBIG. */

/* our additional keys in the metadata list; note that sqlite only
** uses values up to 15 and it's not likely to use more as that would
** require a change in file format */
#define LUMO_LMDB_META_COUNTER   101
#define LUMO_LMDB_TRANSACTION    102

/* constants for pragma lmdb_transaction; default is 0 i.e. optimistic */
#define LMDB_TRANSACTION_OPTIMISTIC 0
#define LMDB_TRANSACTION_NOUPGRADE  1
#define LMDB_TRANSACTION_SERIALISE  2

/* newer versions of sqlite3 use this during an "insert from select"; if
** not defined, we define it as zero which will skip any optimised code
** depending on it */
#ifndef BTREE_PREFORMAT
#define BTREE_PREFORMAT 0
#endif

/* we implement savepoints as nested transactions: this is the stack */
typedef struct BtreeSavepoint BtreeSavepoint;
struct BtreeSavepoint {
  BtreeSavepoint *prev;
  MDB_txn *txn;
  int nSavepoint;
};

struct Btree {
  sqlite3 *db;
  sqlite3_vfs *pVfs;
  MDB_env *env;
  BtreeSavepoint *svp;
  BtCursor *first_cursor;
  BtCursor *last_cursor;
  MDB_dbi dataDbi;
  void * pSchema;
  void (*xFreeSchema)(void *);
  int flags;
  int vfsFlags;
  int mdbFlags;
  int iDataVersion;
  u64 dataVersionCounter;
  u8 dataVersionValid;
  u32 nOwnCommit;
  u8 inTrans;
  u8 isTemp;
  u8 isMemdb;
  u8 inBackup;
  u8 hasData;
  u8 transType;
  /* A read-only LMDB txn kept across autocommit reads via mdb_txn_reset/renew
  ** instead of begin/abort per statement: Parked (reset) at
  ** the end of an autocommit read in CommitPhaseTwo, revived (renew) at the
  ** next read in sqlite3BtreeBeginTrans, discarded on write or close.  NULL
  ** when none is parked. */
  MDB_txn *parkedReader;
  /* Single-entry cache for get_table(): formatting "t%08x" and calling
  ** mdb_dbi_open()+mdb_set_compare() on every row of a bulk insert/update is
  ** pure overhead, since the dbi for a given root table is stable for the life
  ** of a transaction.  Cache the last resolved (iTable,flags)->dbi and reuse it
  ** while the savepoint/txn pointer is unchanged.  An entry is used only
  ** while p->svp equals gtCacheSvp; gtCacheValid is also cleared when a new
  ** transaction begins, because a freed savepoint's address can be reused. */
  void *gtCacheSvp;          /* p->svp value the cache entry is valid for */
  unsigned int gtCacheTable; /* iTable of the cached entry */
  int gtCacheFlags;          /* tableFlags of the cached entry */
  MDB_dbi gtCacheDbi;        /* resolved dbi */
  u8 gtCacheValid;
  u8 hasProcessWriteLock;
  /* Set when a blob handle may have a cursor open on p; see
  ** invalidateIncrblobCursors. */
  u8 hasIncrblobCur;
  Btree *nextProcessWriter;
#ifdef LUMO_LMDB_DEBUG
  /* Counts writes (put/del) on this btree.  Used only to assert that a cursor
  ** never reads its cached row across a sibling write without repositioning;
  ** see cursorCacheSet/the payload accessors. */
  unsigned int wrEpoch;
#endif
  void *pEph;                /* EphTree* for BTREE_SINGLE: in-RAM backing, no LMDB */
  char path[];
};

/* Btrees in this process that hold a write transaction, by path.  LMDB
** serialises writers with a process-shared lock, so a second write
** transaction on the same file waits in mdb_txn_begin() for the first to end,
** which cannot happen when both connections run on one thread.  A second
** connection in this process that asks to write the same file, for example a
** second BEGIN EXCLUSIVE, gets SQLITE_BUSY once its busy handler gives up. */
static Btree *lumoProcessWriters = NULL;
static int lumoProcessWriteLock(Btree *p) {
  Btree *q;
  if (p->hasProcessWriteLock) return SQLITE_OK;
  for (q = lumoProcessWriters; q; q = q->nextProcessWriter) {
    if (q != p && strcmp(q->path, p->path) == 0) return SQLITE_BUSY;
  }
  p->nextProcessWriter = lumoProcessWriters;
  lumoProcessWriters = p;
  p->hasProcessWriteLock = 1;
  return SQLITE_OK;
}

static void lumoProcessWriteUnlock(Btree *p) {
  Btree **pp;
  if (!p->hasProcessWriteLock) return;
  for (pp = &lumoProcessWriters; *pp; pp = &(*pp)->nextProcessWriter) {
    if (*pp == p) {
      *pp = p->nextProcessWriter;
      break;
    }
  }
  p->nextProcessWriter = NULL;
  p->hasProcessWriteLock = 0;
}

struct BtCursor {
  Btree * pBtree;
  BtCursor * prev;
  BtCursor * next;
  MDB_dbi dbi;
  MDB_txn *txn;
  MDB_cursor * cursor;
  struct KeyInfo *pKeyInfo;
  Pgno rootPage;
  int tripCode;
  unsigned int hints;
  u8 atEof;
  u8 wrFlag;
  /* Set by sqlite3BtreeClearCursor.  The cursor has no current position, so a
  ** relative move reports EOF.  Any positioning operation clears it. */
  u8 cleared;
  /* Set by sqlite3BtreeIncrblobCursor on the cursor of a blob handle, which
  ** is open on row incrblobRowid.  incrblobTripped is set while the cursor's
  ** tripCode comes from invalidateIncrblobCursors. */
  u8 isIncrblob;
  u8 incrblobTripped;
  i64 incrblobRowid;
  BtCursor * srcCursor;
  i64 srcKey;
  MDB_val savedKey;
  /* Cache of the row the LMDB cursor currently points at, populated by the
  ** cursor-positioning helpers (cursorMove and the Moveto family).  The
  ** payload and integer-key accessors read it instead of issuing a fresh
  ** mdb_cursor_get(MDB_GET_CURRENT).  curValid is cleared whenever the cursor
  ** may have moved or been invalidated. */
  u8 curValid;
  MDB_val curKey;
  MDB_val curData;
#ifdef LUMO_LMDB_DEBUG
  unsigned int wrEpochAtSet; /* pBtree->wrEpoch when this cache was populated */
#endif
  /* Owned copy of the record bytes for an ephemeral (BTREE_SINGLE) btree,
  ** held in the in-RAM EphTree or, when LUMO_NOEPH is set, in LMDB.
  ** sqlite3BtreePayloadFetch hands SQLite a pointer that SQLite caches in
  ** VdbeCursor.aRow and reads from across several opcodes.  An ephemeral btree
  ** can have several OpenDup cursors open at once, and a write through one of
  ** them can free the EphTree node or move the LMDB page that the pointer
  ** points into.  Copying the row here and returning this buffer keeps the
  ** bytes valid until this cursor's next fetch. */
  unsigned char *stableRow;
  u32 stableRowCap;
  void *ephCur;             /* EphCursor* when pBtree->pEph set */
};

/* Record the row the LMDB cursor is now positioned on so the payload / key
** accessors can reuse it without another mdb_cursor_get. */
static void cursorCacheSet(BtCursor *pCur, const MDB_val *key, const MDB_val *data) {
  pCur->curKey = *key;
  pCur->curData = *data;
  pCur->curValid = 1;
  pCur->cleared = 0;
#ifdef LUMO_LMDB_DEBUG
  if (pCur->pBtree) pCur->wrEpochAtSet = pCur->pBtree->wrEpoch;
#endif
}
static void cursorCacheClear(BtCursor *pCur) {
  pCur->curValid = 0;
}
/* Debug-only: abort if a cursor reads its cached row after a write on the
** same btree that this cursor did not follow with a repositioning. */
#ifdef LUMO_LMDB_DEBUG
#define LUMO_ASSERT_CACHE_FRESH(pCur)                                          \
  do {                                                                          \
    if ((pCur)->pBtree && (pCur)->wrEpochAtSet != (pCur)->pBtree->wrEpoch) {    \
      fprintf(stderr, "LUMO BUG: stale cursor cache read on %p after %u "       \
              "sibling write(s); restore sibling invalidation\n",              \
              (void *)(pCur),                                                   \
              (pCur)->pBtree->wrEpoch - (pCur)->wrEpochAtSet);                  \
      abort();                                                                  \
    }                                                                           \
  } while (0)
#else
#define LUMO_ASSERT_CACHE_FRESH(pCur) do { } while (0)
#endif
/* Clear the writing cursor's cached row after a put or delete through it.
** Sibling cursors keep their cached rows.  Under LUMO_LMDB_DEBUG, the wrEpoch
** check aborts if a sibling reads its cache after this write without
** repositioning. */
static void cursorCacheClearAfterWrite(BtCursor *pCur) {
#ifdef LUMO_LMDB_DEBUG
  if (pCur->pBtree) pCur->pBtree->wrEpoch++;
#endif
  cursorCacheClear(pCur);
}

/* Return a pointer to a copy of [rec,nRec) owned by pCur.  Used for ephemeral
** (BTREE_SINGLE) btrees, where a write through another cursor on the same
** btree can free the EphTree node or, when LUMO_NOEPH is set, move the LMDB
** page that a raw pointer points into.  For other btrees the raw pointer
** is returned and nothing is copied. */
static const unsigned char *cursorStableRow(BtCursor *pCur,
                                            const unsigned char *rec, u32 nRec) {
  if (!pCur->pBtree || !(pCur->pBtree->flags & BTREE_SINGLE)) return rec;
  if (nRec > pCur->stableRowCap) {
    unsigned char *q = sqlite3Realloc(pCur->stableRow, nRec ? nRec : 1);
    if (!q) return rec;            /* on allocation failure, use the raw pointer */
    pCur->stableRow = q;
    pCur->stableRowCap = nRec;
  }
  if (nRec) memcpy(pCur->stableRow, rec, nRec);
  return pCur->stableRow;
}

static const unsigned char *cursorMaybeStableRow(BtCursor *pCur,
                                                 const unsigned char *rec, u32 nRec) {
  if (!(pCur->pBtree->flags & BTREE_SINGLE)) return rec;
  return cursorStableRow(pCur, rec, nRec);
}

/* in-RAM ephemeral backing (lumo_ephemeral.c, #included after this file).
** This marker tells the sqlite3-recipe wrapper (src/lumo_btree.c) that a real
** backend btree is in effect and the ephemeral shim should be included; the
** native (no-backend) build never defines it, so the wrapper skips the shim. */
#define LUMO_HAVE_EPHEMERAL_BACKEND 1
struct EphNode {
  struct EphNode *left, *right;
  int height;
  i64 iKey;                 /* integer rowid, used only for intkey (table) trees */
  u32 nRec;
  unsigned char rec[1];     /* nRec bytes: index key record, or table row data */
};
typedef struct EphNode EphNode;
struct EphTree {
  EphNode *root;
  i64 nEntry;
  unsigned int version;
  struct KeyInfo *pKeyInfo; /* NULL for an intkey (rowid-keyed table) tree */
  int isIntKey;             /* nonzero => ordered by EphNode.iKey, not a record */
};
typedef struct EphTree EphTree;
struct EphCursor {
  EphTree *tree;
  EphNode *cur;
  EphNode *stack[64];
  int sp;
  int dir;                  /* iteration direction the stack is built for:
                            ** +1 forward (ephNext), -1 reverse (ephPrev) */
  int seekAfterDelete;       /* relative move should seek from deleted key */
  unsigned int seenVersion;
  i64 deletedIKey;
  unsigned char *deletedRec;
  u32 nDeletedRec;
};
typedef struct EphCursor EphCursor;
static int ephCursorOpen(Btree *, BtCursor *, struct KeyInfo *);
static void ephCursorClose(BtCursor *);
static void ephTreeFree(Btree *);
static int ephInsert(BtCursor *, const unsigned char *, u32, UnpackedRecord *);
static int ephInsertIntKey(BtCursor *, i64, const unsigned char *, u32);
static int ephSeekInt(struct EphCursor *, i64, int *);
static i64 ephIntKey(struct EphCursor *);
static int ephSeek(struct EphCursor *, UnpackedRecord *, int *);
static void ephFirst(struct EphCursor *, int *);
static int ephNext(struct EphCursor *, int *);
static void ephLast(struct EphCursor *, int *);
static int ephPrev(struct EphCursor *, int *);
static const unsigned char *ephPayload(BtCursor *, u32 *);
static int ephDelete(struct EphCursor *);
static i64 ephCount(struct EphTree *);
static void ephClear(struct EphTree *);

static struct BtCursor fakeCursor = {
  .atEof = 1,
  .cursor = NULL,
  .tripCode = SQLITE_OK,
};

#ifdef LUMO_LMDB_DEBUG
static FILE * _log_fp = NULL;
static void lumo_log_open(void) {
    if (! _log_fp) {
	const char * name = getenv("LUMO_LMDB_DEBUG");
	if (! name) name = "/tmp/lumo.lmdb.debug";
	_log_fp = fopen(name, "a");
    }
}
#define LUMO_LOG(...) do { \
  lumo_log_open(); \
  fprintf(_log_fp, __VA_ARGS__); \
  fflush(_log_fp); \
 } while(0)
static void LUMO_LOG_DATA(int n, const unsigned char * d) {
  int i;
  lumo_log_open();
  for (i = 0; i < n; i++) fprintf(_log_fp, " %02x", d[i]);
  fprintf(_log_fp, " [");
  for (i = 0; i < n; i++)
    fprintf(_log_fp, "%c", d[i] >= 32 && d[i] < 127 ? d[i] : 32);
  fprintf(_log_fp, "]");
  fflush(_log_fp);
}
static u64 get8(const MDB_val *);
static void LUMO_LOG_CURSOR(MDB_cursor * cursor, int isIndex) {
  MDB_val key, data;
  int rc = mdb_cursor_get(cursor, &key, &data, MDB_FIRST);
  lumo_log_open();
  while (rc == 0) {
    unsigned char * d = key.mv_data;
    int i;
    fprintf(_log_fp, "  => %zd:", (size_t)key.mv_size);
    for (i = 0; i < 8; i++) fprintf(_log_fp, "%02x", d[i]);
    if (! isIndex) fprintf(_log_fp, " == %lld", (long long)get8(&key));
    d = data.mv_data;
    for (i = 0; i < data.mv_size; i++) fprintf(_log_fp, " %02x", d[i]);
    fprintf(_log_fp, "   [");
    for (i = 0; i < data.mv_size; i++)
      fprintf(_log_fp, "%c", d[i] >= 32 && d[i] < 127 ? d[i] : 32);
    fprintf(_log_fp, "]\n");
    rc = mdb_cursor_get(cursor, &key, &data, MDB_NEXT);
  }
  fflush(_log_fp);
}
#define LUMO_TXN(i) (\
  (i) == SQLITE_TXN_NONE ? "NONE" : \
  ((i) == SQLITE_TXN_READ ? "READ" : \
  ((i) == SQLITE_TXN_WRITE ? "WRITE" : "?")) \
)
#else
#define LUMO_LOG(fmt, ...) do { } while(0)
#define LUMO_LOG_DATA(n, d) do { } while(0)
#define LUMO_LOG_CURSOR(cursor, isIndex) do { } while(0)
#define LUMO_TXN(i) ""
#undef LUMO_LMDB_LOG_INSERTS
#endif

/* convert 64 bit integers to/from a string which sorts like a number;
** LMDB's integer keys don't appear to be particularly portable if one
** copies things across systems, and the only 64 bit option requires
** either a 64 bit processor or build options which we cannot guarantee
** will be present if we use a lmdb already installed on the system
**
** put8 must be passed a pre-allocated buffer of length at least 8
** and the pointer in d will be a small offset into that buffer
*/
#ifdef LUMO_LMDB_FIXED_ROWID
/* rowids in table keys are stored as fixed length, 8 bytes numbers in
** big endian byte order, so they sort correctly when interpreted as
** strings */
static u64 get8(const MDB_val *d) {
  const unsigned char * ptr = d->mv_data;
  u64 res = 0;
  if (d->mv_size != 8) return res;
  res = (u64)sqlite3Get4byte(ptr);
  res <<= 32;
  return res | (u64)sqlite3Get4byte(ptr + 4);
}

static void put8(MDB_val *d, unsigned char *b, u64 v) {
  sqlite3Put4byte(b, (u32)(v >> 32));
  sqlite3Put4byte(b + 4, (u32)(v & 0xffffffffUL));
  d->mv_size = 8;
  d->mv_data = b;
}
#else
/* rowids in table keys are stored big-endian in the fewest bytes that hold
** the rowid as a u64, at least one byte, so a negative rowid takes 8 bytes.
** rowid_compare() decodes both keys and compares them as signed integers. */
static u64 get8(const MDB_val *d) {
  const unsigned char * ptr = d->mv_data;
  u64 res = 0;
  int i, len = d->mv_size;
  if (len > 8) len = 8;
  for (i = 0; i < len; i++) {
    res <<= 8;
    res |= ptr[i];
  }
  return res;
}

static void put8(MDB_val *d, unsigned char *b, u64 v) {
  int size = 0;
  b += 8;
  /* do-while so that zero encodes as one byte: LMDB rejects a zero-length key */
  do {
    *--b = v & 0xff;
    size++;
    v >>= 8;
    assert (size <= 8);
  } while (v);
  d->mv_size = size;
  d->mv_data = b;
}
#endif

/* taken from sqlightning; sometimes there isn't an exact map of LMDB errors to
** sqlite errors but at least we can report that something went wrong */
static int error_map(int rc) {
  switch(rc) {
    case 0:
      return SQLITE_OK;
    case EACCES:
      return SQLITE_READONLY;
    case EBUSY:
      return SQLITE_BUSY;
    case EIO:
    case MDB_PANIC:
      return SQLITE_IOERR;
    case EPERM:
      return SQLITE_PERM;
    case ENOMEM:
      return SQLITE_NOMEM_BKPT;
    case ENOENT:
      return SQLITE_CANTOPEN_BKPT;
    case ENOSPC:
    case MDB_MAP_FULL:
      return SQLITE_FULL;
    case MDB_NOTFOUND:
      return SQLITE_NOTFOUND;
    case MDB_VERSION_MISMATCH:
    case MDB_INVALID:
      return SQLITE_NOTADB;
    case MDB_PAGE_NOTFOUND:
    case MDB_CORRUPTED:
      return SQLITE_CORRUPT_BKPT;
    case MDB_INCOMPATIBLE:
      return SQLITE_SCHEMA;
    case MDB_BAD_RSLOT:
      return SQLITE_MISUSE_BKPT;
    case MDB_BAD_TXN:
      return SQLITE_ABORT;
    case MDB_BAD_VALSIZE:
      return SQLITE_TOOBIG;
    case MDB_KEYEXIST:
      /* This file passes neither MDB_NOOVERWRITE nor MDB_NODUPDATA, and
      ** sqlite3BtreeInsert retries the MDB_KEYEXIST that MDB_APPEND returns.
      ** Any other duplicate-key report becomes a constraint violation. */
      return SQLITE_CONSTRAINT;
    case MDB_READERS_FULL:
    case MDB_TLS_FULL:
      /* Reader table full (MDB_READERS_FULL), or on Windows no TLS key free
      ** (MDB_TLS_FULL).  A retry can succeed once another reader or
      ** environment releases its slot or key. */
      return SQLITE_BUSY;
    case MDB_DBS_FULL:
    case MDB_TXN_FULL:
    case MDB_CURSOR_FULL:
    case MDB_PAGE_FULL:
    case MDB_MAP_RESIZED:
      /* Too many open databases or dirty pages, data written by another
      ** process beyond this environment's map size, or an internal LMDB
      ** error (CURSOR_FULL, PAGE_FULL). */
      return SQLITE_FULL;
    case MDB_BAD_DBI:
      return SQLITE_ERROR;
  }
  LUMO_LOG("error_map: UNMAPPED mdb code %d (%s) -> SQLITE_INTERNAL\n",
           rc, mdb_strerror(rc));
  return SQLITE_INTERNAL;
}

/* get an integer from the "dataDbi" table, which is open to the metadata;
** the index is assumed to be between 0 and 15 or one of our extra metadata
** index values */
static int get_meta_32_direct(MDB_txn * txn, MDB_dbi dbi, unsigned int idx) {
  MDB_val key, data;
  int rc;
  unsigned char bidx = idx;
  key.mv_data = &bidx;
  key.mv_size = 1;
  rc = mdb_get(txn, dbi, &key, &data);
  if (rc) return 0;
  if (data.mv_size != 4) return 0;
  return sqlite3Get4byte(data.mv_data);
}
static int get_meta_32(Btree *p, unsigned int idx) {
  if (! p->hasData) return 0;
  return get_meta_32_direct(p->svp->txn, p->dataDbi, idx);
}
static int get_meta_64(Btree *p, unsigned int idx) {
  MDB_val key, data;
  int rc;
  unsigned char bidx = idx;
  if (! p->hasData) return 0;
  key.mv_data = &bidx;
  key.mv_size = 1;
  rc = mdb_get(p->svp->txn, p->dataDbi, &key, &data);
  if (rc) return 0;
  return get8(&data);
}

/* put an integer into the "dataDbi" table, which is open to the metadata;
** the index is assumed to be between 0 and 15 or one of our extra metadata
** index values */
static int put_meta_32_direct(MDB_txn * txn, MDB_dbi dbi, unsigned int idx, u32 iValue) {
  char buffer[4];
  unsigned char bidx = idx;
  MDB_val key, data;
  key.mv_data = &bidx;
  key.mv_size = 1;
  data.mv_data = buffer;
  data.mv_size = 4;
  sqlite3Put4byte((u8*)buffer, iValue);
  return mdb_put(txn, dbi, &key, &data, 0);
}
static int put_meta_32(Btree *p, unsigned int idx, u32 iValue) {
  if (! p->hasData) return EACCES;
  return put_meta_32_direct(p->svp->txn, p->dataDbi, idx, iValue);
}
static int put_meta_32_notxn(Btree *p, unsigned int idx, u32 iValue) {
  MDB_dbi dbi;
  MDB_txn *abort_txn = NULL, *use_txn;
  int rc = 0;
  if (p->inTrans == SQLITE_TXN_WRITE) {
    /* We can use the existing transaction */
    dbi = p->dataDbi;
    use_txn = p->svp->txn;
  } else {
    if (p->vfsFlags & SQLITE_OPEN_READONLY) return SQLITE_READONLY;
    if (p->inTrans == SQLITE_TXN_READ) {
      /* read transaction held: upgrading would mirror the BEGIN path; we
      ** decline the upgrade here and report BUSY */
      return SQLITE_BUSY;
    } else {
      rc = mdb_txn_begin(p->env, NULL, 0, &abort_txn);
      if (rc) goto error;
      rc = mdb_dbi_open(abort_txn, "meta", 0, &dbi);
      if (rc) goto error;
      use_txn = abort_txn;
    }
  }
  rc = put_meta_32_direct(use_txn, dbi, idx, iValue);
  if (rc) goto error;
  /* Commit only a transaction made here: the connection commits its own. */
  if (abort_txn) {
    rc = mdb_txn_commit(abort_txn);
    abort_txn = NULL;
    if (rc) goto error;
  }
  return SQLITE_OK;
error:
  if (abort_txn) mdb_txn_abort(abort_txn);
  return error_map(rc);
}
static int put_meta_64(Btree *p, unsigned int idx, u64 uValue) {
  char buffer[8];
  unsigned char bidx = idx;
  MDB_val key, data;
  if (! p->hasData) return EACCES;
  key.mv_data = &bidx;
  key.mv_size = 1;
  put8(&data, (u8*)buffer, uValue);
  return mdb_put(p->svp->txn, p->dataDbi, &key, &data, 0);
}

/* Translate a sync level ("off", "normal", "full" or "extra", lower case,
** or "0" to "3") into LMDB sync flags to clear and set.  The level comes
** from PRAGMA lumo_sync, LUMO_SYNC, LUMO_LMDB_SYNC_DEFAULT or the built-in
** "normal".  Returns -1 for a NULL, empty or unknown level.  LMDB fsyncs
** the data pages and then the meta page on every commit.  Two flags relax
** these fsyncs:
**   MDB_NOMETASYNC : sync data, skip the meta fsync.  A crash can lose only the
**                    single most recent transaction; the database is never
**                    corrupted.  This matches SQLite synchronous=NORMAL.
**   MDB_NOSYNC     : skip all syncs.  A crash can corrupt the database.  This
**                    matches SQLite synchronous=OFF.
** full/extra keep both syncs (no flag), matching SQLite synchronous=FULL. */
static int lumo_sync_flags(const char *level, unsigned int *clr, unsigned int *set) {
  *clr = MDB_NOSYNC | MDB_NOMETASYNC; *set = 0;
  if (!level || !*level) return -1;
  if (!strcmp(level, "full") || !strcmp(level, "extra") || !strcmp(level, "2")
      || !strcmp(level, "3")) { *set = 0; return 0; }
  if (!strcmp(level, "normal") || !strcmp(level, "1")) {
    *set = MDB_NOMETASYNC; return 0;
  }
  if (!strcmp(level, "off") || !strcmp(level, "0")) {
    *set = MDB_NOSYNC; return 0;
  }
  return -1;
}
/* Apply a sync level to an open env.  Clears both relax-flags, then sets the
** one implied by `level`.  Returns SQLITE_OK on success. */
static int lumo_apply_sync(Btree *p, const char *level) {
  unsigned int clr, set;
  int rc;
  if (!p || !p->env) return SQLITE_OK;
  if (lumo_sync_flags(level, &clr, &set) < 0) return SQLITE_ERROR;
  rc = mdb_env_set_flags(p->env, clr, 0);
  if (rc == 0 && set) rc = mdb_env_set_flags(p->env, set, 1);
  LUMO_LOG("lumo sync level=%s clr=%x set=%x rc=%d\n", level, clr, set, rc);
  return rc ? SQLITE_ERROR : SQLITE_OK;
}
#if defined(LUMO_LMDB_SYNC)
/* Open-time sync level, in precedence order: the LUMO_SYNC env var, then the
** compile-time -DLUMO_LMDB_SYNC_DEFAULT, then "normal". */
static const char *lumo_sync_open_level(void) {
  const char *lv = getenv("LUMO_SYNC");
#ifdef LUMO_LMDB_SYNC_DEFAULT
  if (!lv || !*lv) lv = LUMO_LMDB_SYNC_DEFAULT;
#endif
  if (!lv || !*lv) lv = "normal";
  return lv;
}
#endif /* LUMO_LMDB_SYNC */


/* parse backend pragmas */
int lumo_backend_pragma(
	sqlite3 * db,           /* database connection */
	const char * zDb,       /* database name */
	const char * zLeft,     /* pragma ID */
	const char * zRight,    /* value or NULL */
	const char ** zResult   /* string result value or error message */
){
  *zResult = 0;
  if (strcmp(zLeft, "lmdb_transaction") == 0) {
    Btree * p = db->aDb[0].pBt;
    if (zRight) {
	int rc;
	u32 trans;
	/* set the transaction type */
	if (strcmp(zRight, "optimistic") == 0) {
	  trans = LMDB_TRANSACTION_OPTIMISTIC;
	} else if (strcmp(zRight, "noupgrade") == 0) {
	  trans = LMDB_TRANSACTION_NOUPGRADE;
	} else if (strcmp(zRight, "serialise") == 0 || strcmp(zRight, "serialize") == 0) {
	  trans = LMDB_TRANSACTION_SERIALISE;
	} else {
	  *zResult = "Invalid transaction type: use optimistic, noupgrade or serialise";
	  return SQLITE_ERROR;
	}
	rc = put_meta_32_notxn(p, LUMO_LMDB_TRANSACTION, trans);
	if (rc == SQLITE_OK) p->transType = trans;
	return rc;
    } else {
	/* get the transaction type */
	if (p->transType == LMDB_TRANSACTION_OPTIMISTIC) {
	  *zResult = "optimistic";
	} else if (p->transType == LMDB_TRANSACTION_NOUPGRADE) {
	  *zResult = "noupgrade";
	} else if (p->transType == LMDB_TRANSACTION_SERIALISE) {
	  *zResult = "serialise";
	} else {
	  *zResult = "Internal error, invalid value stored in database";
	  return SQLITE_CORRUPT_BKPT;
	}
	return SQLITE_OK;
    }
  }
#if defined(LUMO_LMDB_SYNC)
  if (strcmp(zLeft, "lumo_sync") == 0) {
    Btree * p = db->aDb[0].pBt;
    if (zRight) {
      unsigned int clr, set;
      if (lumo_sync_flags(zRight, &clr, &set) < 0) {
        *zResult = "lumo_sync must be one of: full, normal, off";
        return SQLITE_ERROR;
      }
      return lumo_apply_sync(p, zRight);
    } else {
      /* report the env's current relax-flags as a level */
      unsigned int f = 0;
      if (p && p->env) mdb_env_get_flags(p->env, &f);
      if      (f & MDB_NOSYNC)     *zResult = "off";
      else if (f & MDB_NOMETASYNC) *zResult = "normal";
      else                          *zResult = "full";
      return SQLITE_OK;
    }
  }
#endif
  return SQLITE_NOTFOUND;
}

/* Compare rowids as signed integers while preserving their existing encoding. */
static int rowid_compare(const MDB_val *a, const MDB_val *b) {
  i64 av = (i64)get8(a);
  i64 bv = (i64)get8(b);
  return av < bv ? -1 : av > bv;
}

/* forward decl: defined with the spill helpers below */
static int lumoNeedsSpill(struct KeyInfo *pKeyInfo);

/* vdbeRecordCompareInt() for a stored key shorter than that routine's
** eight-byte read.  It reads only the bytes the leading field's serial type
** occupies, and passes every other case to the general routine.  The caller
** has run sqlite3VdbeFindCompare(), which sets p->r1 and p->r2. */
static int lumoRecordCompareIntShort(int nKey1, const void *pKey1,
                                     UnpackedRecord *p) {
  static const unsigned char aWidth[10] = { 0, 1, 2, 3, 4, 6, 8, 0, 0, 0 };
  const unsigned char *k = (const unsigned char *)pKey1;
  const unsigned char *d;
  int st;
  i64 lhs, v;
  u32 y;
  u64 x;
  if (nKey1 < 2) return sqlite3VdbeRecordCompare(nKey1, pKey1, p);
  st = k[1];
  if (st > 9 || st == 0 || st == 7 || (k[0] & 0x3F) + aWidth[st] > nKey1) {
    return sqlite3VdbeRecordCompare(nKey1, pKey1, p);
  }
  d = k + (k[0] & 0x3F);
  switch (st) {
    case 1: lhs = (signed char)d[0]; break;
    case 2: lhs = 256 * (signed char)d[0] | d[1]; break;
    case 3: lhs = 65536 * (signed char)d[0] | (d[1] << 8) | d[2]; break;
    case 4:
      y = ((u32)d[0] << 24) | ((u32)d[1] << 16) | ((u32)d[2] << 8) | d[3];
      lhs = (y & 0x80000000u) ? (i64)y - ((i64)1 << 32) : (i64)y;
      break;
    case 5:
      y = ((u32)d[2] << 24) | ((u32)d[3] << 16) | ((u32)d[4] << 8) | d[5];
      lhs = (i64)y + ((i64)1 << 32) * (256 * (signed char)d[0] | d[1]);
      break;
    case 6:
      x = ((u64)d[0] << 56) | ((u64)d[1] << 48) | ((u64)d[2] << 40)
        | ((u64)d[3] << 32) | ((u64)d[4] << 24) | ((u64)d[5] << 16)
        | ((u64)d[6] << 8) | d[7];
      memcpy(&lhs, &x, sizeof lhs);
      break;
    default: lhs = st - 8; break;
  }
  v = p->aMem[0].u.i;
  if (v > lhs) return p->r1;
  if (v < lhs) return p->r2;
  if (p->nField > 1) return sqlite3VdbeRecordCompareWithSkip(nKey1, pKey1, p, 1);
  p->eqSeen = 1;
  return p->default_rc;
}

/* Compare a key passed to an LMDB function against one stored in the database.
** LMDB passes the unpacked search key in key[1], placed there by
** sqlite3BtreeInsert and the other functions that drive key comparisons. */
static int index_compare_impl(const MDB_val *a, const MDB_val *b, int spills) {
  UnpackedRecord *p = a[1].mv_data;
  int rc;
  /* LMDB calls this comparator throughout B-tree descent, comparing the search
  ** key against internal branch-page pivot keys, which can include stale
  ** separators for deleted rows, as well as against leaf entries.
  ** sqlite3VdbeRecordCompare has two side effects that are correct only when
  ** the comparator runs once, at the final leaf entry, as in SQLite's own
  ** b-tree:
  **
  **   - it sets pPKey->eqSeen = 1 on an equal comparison.  A stale branch pivot
  **     can equal the search key when no live leaf row matches, and a comparator
  **     that sets eqSeen there makes OP_SeekGE's eqOnly fast-path (used for
  **     "WHERE pk = ?") act on a neighbouring row.
  **
  **   - the pPKey->default_rc tie-break (returned when one key is a prefix of
  **     the other) is meaningful only relative to a leaf entry.
  **
  ** So this comparator is a pure ordering function.  It snapshots and restores
  ** eqSeen so descent comparisons cannot set it, and IndexMoveto sets eqSeen
  ** from the landed leaf.
  **
  ** default_rc is set to 0 only when the search record carries the full key
  ** (p->nField >= nKeyField).  Two distinct complete primary keys are never a
  ** prefix of one another, so this only collapses the all-equal case, which is
  ** what a full-key lookup wants.  A prefix seek (p->nField < nKeyField, e.g.
  ** "WHERE a = ?" on PRIMARY KEY(a,b)) is a prefix of every stored (a, *) key,
  ** and default_rc carries the seek direction, so it is kept and the search
  ** lands at the edge of the group.  `spills` matches the storage path
  ** (lumoCursorSpills).  An ephemeral btree stores whole records and uses plain
  ** ordering. */
  u8 savedEq = p->eqSeen;
  /* Use SQLite's specialised record comparator instead of the fully general
  ** sqlite3VdbeRecordCompare().  LMDB invokes this callback at every level of
  ** every B-tree descent (a binary search of each page on the path), and the
  ** general routine re-parses the stored key's serial-type header in full on
  ** each call.  sqlite3VdbeFindCompare() returns vdbeRecordCompareInt /
  ** vdbeRecordCompareString for the common case where the first index column is
  ** an integer or a (BINARY-collated) string; those decode only the leading
  ** field of the stored key inline and return as soon as it differs, which is
  ** the overwhelmingly common outcome during descent.  They share the exact
  ** r1/r2/default_rc/eqSeen semantics of the general routine (they fall back to
  ** it whenever the leading fields tie or the key shape is unsupported), so the
  ** eqSeen snapshot/restore and default_rc neutralisation below remain correct
  ** regardless of which concrete comparator runs.  FindCompare also (re)caches
  ** p->u/r1/r2 from p->aMem[0] on every call, so it is safe to call per compare
  ** even though it mutates p.
  **
  ** The specialised comparators rely on SQLite's guarantee of at least 74 bytes
  ** of padding after every buffer it hands them (see sqlite3VdbeFindCompare()),
  ** and from SQLite 3.54 vdbeRecordCompareInt() reads eight bytes at
  ** key[key[0] & 0x3F] whatever the leading field's width.  An LMDB key points
  ** into a page and has no padding after it, so a key ending at the end of a
  ** page would be read past the page.  A stored key shorter than that eight-byte
  ** read goes to lumoRecordCompareIntShort() when the leading field is an
  ** integer, and to the general routine otherwise. */
  RecordCompare xCompare = sqlite3VdbeFindCompare(p);
  if (b->mv_size == 0) {
    xCompare = sqlite3VdbeRecordCompare;
  } else if (b->mv_size < (size_t)(((const unsigned char *)b->mv_data)[0] & 0x3F) + 8) {
    xCompare = (xCompare != sqlite3VdbeRecordCompare && (p->aMem[0].flags & MEM_Int))
               ? lumoRecordCompareIntShort : sqlite3VdbeRecordCompare;
  }
  if (spills && lumoNeedsSpill(p->pKeyInfo)
      && p->nField >= p->pKeyInfo->nKeyField) {
    i8 savedRc = p->default_rc;
    u16 savedField = p->nField;
    p->default_rc = 0;
    /* The stored key holds only the nKeyField ordering fields.  Comparing
    ** more fields would run past its end, which the record comparator
    ** reports as corruption. */
    p->nField = p->pKeyInfo->nKeyField;
    rc = -xCompare(b->mv_size, b->mv_data, p);
    p->nField = savedField;
    p->default_rc = savedRc;
  } else {
    rc = -xCompare(b->mv_size, b->mv_data, p);
  }
  p->eqSeen = savedEq;
  return rc;
}

/* Comparator for persistent index / WITHOUT ROWID tables, which may spill. */
static int index_compare(const MDB_val *a, const MDB_val *b) {
  return index_compare_impl(a, b, 1);
}

/* Comparator for ephemeral index btrees (sorter, DISTINCT, automatic index)
** stored in LMDB when LUMO_NOEPH is set.  They store the whole record as the
** key and so do not neutralise default_rc. */
static int index_compare_temp(const MDB_val *a, const MDB_val *b) {
  return index_compare_impl(a, b, 0);
}

/* Spill-to-value record format for index / WITHOUT ROWID tables 
**
** The LMDB key carries only the comparison columns (pKeyInfo->nKeyField
** fields); the LMDB value carries the full record for the payload accessors.
** For an index with nKeyField == nAllField the key is the whole record and
** the value is just varint(0); a UNIQUE index whose columns are all NOT NULL
** has nKeyField < nAllField and spills like a WITHOUT ROWID table.  For a
** WITHOUT ROWID table the key is only the primary-key columns, so the other
** columns do not count towards LMDB's key-length cap.  A primary key longer
** than the cap still makes the insert return SQLITE_TOOBIG.
**
** Value layout:
**     varint(nRec) || recBytes[nRec] || varint(0) || lumoTrailer?
** when spilling, or just "varint(0) || trailer" otherwise.
** The trailing varint(0) is a placeholder payload accessors do not return;
** a trailer follows it only when the btree flags include
** BTREE_LUMO_EXTENSIONS, which is never set.  An index entry's rowsum is a
** column of the record itself, appended by lumoExtensionAdd() in
** OP_IdxInsert.
**
** Format is chosen per table from the schema (lumoNeedsSpill); see
** lumoIndexRecord() below. */

/* True if the cursor's KeyInfo has fewer key fields than fields
** (nKeyField < nAllField): a WITHOUT ROWID table whose record carries
** non-key columns, or a UNIQUE index whose columns are all NOT NULL. */
static int lumoNeedsSpill(struct KeyInfo *pKeyInfo) {
  return pKeyInfo && pKeyInfo->nKeyField < pKeyInfo->nAllField;
}

/* Cursor-aware spill decision.  Spilling means the LMDB key holds the
** nKeyField primary-key columns and the value holds the full record.  It suits
** a persistent WITHOUT ROWID table, whose primary-key columns are unique.  An
** ephemeral btree (opened with BTREE_SINGLE) that sorts rows for ORDER BY
** also has nKeyField < nAllField, but its key columns are not unique: each
** record continues with a sequence number and the result columns.  Such a
** btree stores the whole record as the key.  The test is BTREE_SINGLE rather
** than the database-level temp flag, because a temp or :memory: database still
** holds ordinary WITHOUT ROWID tables that spill. */
static int lumoCursorSpills(BtCursor *pCur) {
  return pCur && pCur->pKeyInfo
      && !(pCur->pBtree && (pCur->pBtree->flags & BTREE_SINGLE))
      && lumoNeedsSpill(pCur->pKeyInfo);
}

/* sqlite3VdbeRecordCompare() of a key stored under pCur against pIdxKey.
** When pCur spills, the stored key holds only the nKeyField ordering fields,
** so the comparison stops after nKeyField fields of pIdxKey.  Comparing more
** would run past the end of the stored key, which the record comparator
** reports as corruption. */
static int lumoStoredKeyCompare(BtCursor *pCur, int nKey, const void *pKey,
                                UnpackedRecord *pIdxKey) {
  u16 nField = pIdxKey->nField;
  int cmp;
  if (lumoCursorSpills(pCur) && nField > pCur->pKeyInfo->nKeyField)
    pIdxKey->nField = pCur->pKeyInfo->nKeyField;
  cmp = sqlite3VdbeRecordCompare(nKey, pKey, pIdxKey);
  pIdxKey->nField = nField;
  return cmp;
}

/* Truncate a serialised SQLite record to its first nField columns,
** producing another well-formed serialised record.
**
** A record is:  varint(headerLen) || serialType[0..] || body[0..]
** headerLen counts itself.  Each serial type is a varint.  The body holds
** the column values back-to-back in the same order.  To keep the first
** nField columns we:
**   1. walk the header collecting the first nField serial types and the
**      total body length they occupy;
**   2. emit a new header (its own length recomputed) + those serial types;
**   3. copy the corresponding leading body bytes.
**
** The truncated record sorts identically to the original over its first
** nField fields, which for a WITHOUT ROWID table are exactly the PK columns
** (SQLite serialises a WITHOUT ROWID row with its PK columns first, in PK
** order).
**
** On success writes the new record to *pOut (caller-owned, sqlite3_malloc'd)
** and its length to *pnOut, returning SQLITE_OK.  The caller frees *pOut. */
static int lumoTruncateRecord(const unsigned char *aRec, int nRec,
                              int nField, unsigned char **pOut, int *pnOut) {
  u32 hdrLen = 0;
  int hdrSizeBytes;
  int i;
  u32 off;
  u32 bodyKept = 0;
  u32 hdrKept;            /* bytes of serial-type varints we keep */
  const unsigned char *pHdrStart;
  unsigned char *out;
  u32 newHdrLen, newHdrSizeBytes;
  u32 total;
  unsigned char *w;

  if (nRec < 1) return SQLITE_CORRUPT_BKPT;
  hdrSizeBytes = getVarint32(aRec, hdrLen);
  if (hdrLen > (u32)nRec || hdrSizeBytes < 1) return SQLITE_CORRUPT_BKPT;
  pHdrStart = aRec + hdrSizeBytes;
  off = hdrSizeBytes;
  hdrKept = 0;
  /* Walk serial types, accumulating body sizes for the first nField. */
  for (i = 0; i < nField; i++) {
    u32 serial;
    int n;
    if (off >= hdrLen) {
      /* Fewer fields present than requested: keep what exists. */
      break;
    }
    n = getVarint32(aRec + off, serial);
    if (n < 1) return SQLITE_CORRUPT_BKPT;
    off += n;
    hdrKept += n;
    bodyKept += sqlite3VdbeSerialTypeLen(serial);
  }
  /* New header length: size-of(headerLen varint) + hdrKept.  headerLen is
  ** self-counting, so compute its varint width iteratively. */
  newHdrSizeBytes = 1;
  do {
    newHdrLen = newHdrSizeBytes + hdrKept;
    newHdrSizeBytes = sqlite3VarintLen(newHdrLen);
  } while (newHdrSizeBytes + hdrKept != newHdrLen);

  total = newHdrLen + bodyKept;
  out = sqlite3_malloc(total ? total : 1);
  if (!out) return SQLITE_NOMEM_BKPT;
  w = out;
  w += putVarint32(w, newHdrLen);
  memcpy(w, pHdrStart, hdrKept);
  w += hdrKept;
  /* Body of the kept fields starts right after the full header. */
  memcpy(w, aRec + hdrLen, bodyKept);
  *pOut = out;
  *pnOut = (int)total;
  return SQLITE_OK;
}

/* Given the LMDB key and value of an index entry, return a pointer to the full
** serialised record and its length.  When `spills` is set the record lives at
** the front of the value, as varint(recLen) followed by the record.  Otherwise
** the record is the key. */
static void lumoIndexRecord(int spills,
                            const MDB_val *key, const MDB_val *data,
                            const unsigned char **pRec, u32 *pnRec) {
  if (spills && data && data->mv_data) {
    u32 recLen = 0;
    int n = getVarint32((const unsigned char *)data->mv_data, recLen);
    *pRec = (const unsigned char *)data->mv_data + n;
    *pnRec = recLen;
  } else {
    *pRec = (const unsigned char *)key->mv_data;
    *pnRec = (u32)key->mv_size;
  }
}

/* Write the sub-database name of table iTable into z, which must hold at least
** 10 bytes: "t" and eight lowercase hex digits, as snprintf("t%08x") would
** write it.  get_table() calls this when its dbi cache misses. */
static void lumoTableName(char *z, unsigned int iTable) {
  static const char hex[] = "0123456789abcdef";
  int i;
  z[0] = 't';
  for (i = 8; i > 0; i--) {
    z[i] = hex[iTable & 0xf];
    iTable >>= 4;
  }
  z[9] = 0;
}

/* open or create a database in the LMDB environment; tableFlags is 0 or a bitwise
 * OR of the following values.  Returns an SQLite result code. */
typedef enum {
  get_table_index  = 1,  /* table is an index */
  get_table_create = 2,  /* create table if it does not exist */
  get_table_nocmp  = 4,  /* do not set a key compare function */
} get_table_t;
static int get_table(Btree *p, unsigned int iTable, get_table_t tableFlags, MDB_dbi *dbi) {
  char dbiName[32];
  int rc;
  unsigned int mdbFlags = 0;
  if (iTable < 1) return SQLITE_CORRUPT_BKPT;
  if (! p->svp) return SQLITE_INTERNAL;
  /* Fast path: reuse the last resolved dbi when nothing relevant changed.
  ** The (iTable -> dbi) mapping is stable within a transaction, so this
  ** avoids the per-row name formatting + mdb_dbi_open name scan + mdb_set_compare
  ** that otherwise dominate bulk insert/update. */
  if (p->gtCacheValid
   && p->gtCacheSvp == (void *)p->svp
   && p->gtCacheTable == iTable
   && p->gtCacheFlags == (int)tableFlags) {
    *dbi = p->gtCacheDbi;
    return SQLITE_OK;
  }
  if ((tableFlags & get_table_create) != 0) mdbFlags |= MDB_CREATE;
  lumoTableName(dbiName, iTable);
  LUMO_LOG("get_table: %d %x => <%s>\n", iTable, tableFlags, dbiName);
  rc = mdb_dbi_open(p->svp->txn, dbiName, mdbFlags, dbi);
  if (rc) return error_map(rc);
  if (tableFlags & get_table_nocmp) return SQLITE_OK;
  if (tableFlags & get_table_index) {
    /* When LUMO_NOEPH is set, ephemeral (BTREE_SINGLE) btrees store the whole
    ** record as the key and use the comparator that leaves default_rc alone.
    ** Persistent index and WITHOUT ROWID tables may spill and use
    ** index_compare. */
    rc = mdb_set_compare(p->svp->txn, *dbi,
                         (p->flags & BTREE_SINGLE) ? index_compare_temp
                                                   : index_compare);
  } else {
    rc = mdb_set_compare(p->svp->txn, *dbi, rowid_compare);
  }
  if (rc == SQLITE_OK) {
    /* Remember this resolution for subsequent same-table calls in this txn. */
    p->gtCacheSvp = (void *)p->svp;
    p->gtCacheTable = iTable;
    p->gtCacheFlags = (int)tableFlags;
    p->gtCacheDbi = *dbi;
    p->gtCacheValid = 1;
  }
  return error_map(rc);
}

/* Invalidate every cursor on p.  A cursor that is not tripped has its
** position saved so it can be restored later; a tripped cursor returns
** tripCode when used.  tripCode SQLITE_OK trips no cursor, and with writeOnly
** nonzero it trips only write cursors.  Every LMDB cursor is closed, because
** ending the transaction frees the cursors LMDB tracks for it.  svp is used
** only for logging. */
static int invalidateCursors(Btree *p,
			     BtreeSavepoint * svp,
			     int tripCode,
			     int writeOnly)
{
  BtCursor *pList;
  int rc = 0;
  if (!p->first_cursor) return 0;
  LUMO_LOG("invalidateCursors(tripCode=%d, writeOnlu=%d, p=%p, svp=%p)\n",
	   tripCode, writeOnly, p, svp);
  pList = p->first_cursor;
  while (pList) {
    BtCursor *pCur = pList;
    pList = pList->next;
    /* Whatever happens below (position saved + cursor closed, or cursor
    ** dropped), this cursor's cached current row becomes invalid: its
    ** pointers reference the transaction that is being torn down. */
    cursorCacheClear(pCur);
    if (pCur->tripCode == SQLITE_OK && pCur->cursor != NULL) {
      if (tripCode == SQLITE_OK || !(pCur->wrFlag || !writeOnly)) {
	/* save cursor position before invalidating */
	MDB_val key, data;
	int rc1 = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
	if (rc1 == EINVAL || rc1 == MDB_NOTFOUND) {
	  /* The cursor is not positioned on a valid row (e.g. it landed
	  ** past the end of an empty table, or a prior seek found nothing).
	  ** There is no position to save: just drop the LMDB cursor so it
	  ** is recreated against the new transaction, leaving savedKey
	  ** empty.  A read transaction can be upgraded while a cursor with no
	  ** valid row is open, so finding one here is not an error. */
	  if (pCur->savedKey.mv_data) {
	    sqlite3DbFree(0, pCur->savedKey.mv_data);
	  }
	  pCur->savedKey.mv_data = NULL;
	  pCur->savedKey.mv_size = 0;
	  mdb_cursor_close(pCur->cursor);
	  pCur->cursor = NULL;
	  if (pCur->wrFlag || !writeOnly)
	    pCur->tripCode = tripCode;
	  continue;
	}
	if (rc1) goto error;
	pCur->savedKey.mv_data = sqlite3DbMallocRaw(0, key.mv_size);
	if (!pCur->savedKey.mv_data) {
	  rc1 = ENOMEM;
	error:
	  pCur->tripCode = error_map(rc1);
	  if (!rc) rc = rc1;
	  mdb_cursor_close(pCur->cursor);
	  pCur->cursor = NULL;
	  continue;
	}
	pCur->savedKey.mv_size = key.mv_size;
	memcpy(pCur->savedKey.mv_data, key.mv_data, key.mv_size);
	/* this cursor will no longer work, we'll have to make a new one */
	mdb_cursor_close(pCur->cursor);
	pCur->cursor = NULL;
      }
      if (pCur->wrFlag || !writeOnly)
	pCur->tripCode = tripCode;
    }
    if (pCur->cursor != NULL) {
      mdb_cursor_close(pCur->cursor);
      pCur->cursor = NULL;
    }
  }
  return rc;
}

/* Trip with SQLITE_ABORT each blob-handle cursor on table iTable of p that is
** open on row iRow, or every one on the table when isClearTable is set.  The
** blob handle then returns SQLITE_ABORT, as it does upstream when its row is
** replaced or deleted or its table is cleared. */
static void invalidateIncrblobCursors(Btree *p, Pgno iTable, i64 iRow,
                                      int isClearTable) {
  BtCursor *pCur;
  p->hasIncrblobCur = 0;
  for (pCur = p->first_cursor; pCur; pCur = pCur->next) {
    if (!pCur->isIncrblob) continue;
    p->hasIncrblobCur = 1;
    if (pCur->tripCode == SQLITE_OK && pCur->rootPage == iTable
        && (isClearTable || pCur->incrblobRowid == iRow)) {
      cursorCacheClear(pCur);
      pCur->tripCode = SQLITE_ABORT;
      pCur->incrblobTripped = 1;
    }
  }
}

/* sqlite3_blob_reopen() moves a blob-handle cursor to another row with
** sqlite3BtreeTableMoveto().  Upstream lets that seek succeed on a cursor
** invalidated by invalidateIncrblobCursors, so clear the trip, and open a new
** LMDB cursor if a savepoint closed the old one without saving a position. */
static int incrblobReseek(BtCursor *pCur) {
  Btree *p = pCur->pBtree;
  int rc;
  pCur->incrblobTripped = 0;
  pCur->tripCode = SQLITE_OK;
  if (pCur->cursor || pCur->savedKey.mv_data) return SQLITE_OK;
  rc = get_table(p, pCur->rootPage, 0, &pCur->dbi);
  if (rc) return rc;
  return error_map(mdb_cursor_open(p->svp->txn, pCur->dbi, &pCur->cursor));
}

/* close all savepoints up to the specified "nTo" (-1 to close all); the
** main transaction is never closed by this function.  If "commit" is
** nonzero, the transaction will be committed otherwise it will be
** rolled back.  All cursors on p are invalidated at each level closed.
** If "tripCode" is SQLITE_OK, their position will be saved
** so they can be restored later.  Otherwise they will return tripCode
** if used */
static int closeAllSavepoints(Btree *p, int nTo, int commit, int tripCode) {
  int rc = 0;
  if (! p->svp || ! p->svp->prev) return 0;
  LUMO_LOG("closeAllSavepoints(nTo=%d, commit=%d, tripCode=%d) %p\n",
	   nTo, commit, tripCode, p);
  while (p->svp->prev && p->svp->nSavepoint != nTo) {
    BtreeSavepoint * S = p->svp;
    int rc1;
    p->svp = S->prev;
    LUMO_LOG("    nSavepoint=%d %p\n", S->nSavepoint, S->txn);
    rc1 = invalidateCursors(p, p->svp, tripCode, 0);
    if (rc1 && ! rc) rc = rc1;
    if (commit && ! rc) {
      rc1 = mdb_txn_commit(S->txn);
      if (rc1 && ! rc) rc = rc1;
    } else {
      mdb_txn_abort(S->txn);
    }
    sqlite3_free(S);
  }
  return rc;
}

/* Lazily materialise nested LMDB transactions for any named savepoints
** that SQLite has BEGUN but for which we haven't yet pushed a stack
** entry.  SQLite's vdbe.c does NOT call sqlite3BtreeSavepoint on
** SAVEPOINT_BEGIN (only on RELEASE/ROLLBACK), so the LMDB backend has
** to detect the BEGIN by polling db->nSavepoint at points where the
** btree is about to be used.  This is called from sqlite3BtreeBeginTrans,
** which SQLite invokes via OP_Transaction at the start of every
** statement.  That's the latest possible moment we can guarantee the
** nested txn exists by the time any write happens.
**
** Statement savepoints (created via sqlite3BtreeBeginStmt) are
** identified by an nSavepoint label >= db->nSavepoint, so they don't
** conflict with named-savepoint labels which run from 0..nSavepoint-1.
** Named entries are pushed on top of the stack.  sqlite3BtreeBeginStmt
** runs after sqlite3BtreeBeginTrans, so when this function runs the stack
** holds only the outer transaction and named savepoints already pushed.
**
** A subsequent ROLLBACK TO s_k or RELEASE s_k arrives via
** sqlite3BtreeSavepoint with iSavepoint = k, and our existing
** closeAllSavepoints walks the stack down to nSavepoint == k.
*/
static int syncNamedSavepoints(Btree *p) {
  int target;
  int have;
  BtreeSavepoint *S;
  if (p->db == NULL) return SQLITE_OK;
  target = p->db->nSavepoint;
  if (target <= 0) return SQLITE_OK;
  /* Count current named savepoint entries.  Stack invariant: outer txn
  ** at the bottom (nSavepoint == -1), then named entries with
  ** nSavepoint 0..N-1.  Statement-savepoint entries (nSavepoint >=
  ** target) only appear during a statement's lifetime and we don't
  ** expect them at this entry point. */
  have = 0;
  for (S = p->svp; S != NULL; S = S->prev) {
    if (S->nSavepoint >= 0 && S->nSavepoint < target) have++;
  }
  while (have < target) {
    BtreeSavepoint *N = sqlite3DbMallocZero(0, sizeof(BtreeSavepoint));
    int rc;
    if (! N) return SQLITE_NOMEM;
    /* nest under the current top-of-stack txn */
    rc = mdb_txn_begin(p->env, p->svp ? p->svp->txn : NULL, 0, &N->txn);
    if (rc) {
      LUMO_LOG("syncNamedSavepoints: mdb_txn_begin -> %d\n", rc);
      sqlite3_free(N);
      return error_map(rc);
    }
    N->nSavepoint = have;
    N->prev = p->svp;
    p->svp = N;
    LUMO_LOG("syncNamedSavepoints: pushed depth=%d txn=%p (parent=%p)\n",
	     have, N->txn, N->prev ? N->prev->txn : NULL);
    have++;
  }
  return SQLITE_OK;
}

/* This LMDB backend has no shared cache, so the setting is ignored. */
int sqlite3_enable_shared_cache(int enable) { return 0; }

/* non-vfs routine to delete a single-file LMDB environment: the data file
** itself and its sibling "<name>-lock" lock file.  To get to the point of
** calling this we must have gone through the creation of a temporary
** database, which needs also be fixed to use vfs calls */
static void removeTempDb(const char *zPath) {
  char buffer[BTREE_MAX_PATH + 8];
  LUMO_LOG("removeTempDb(%s)\n", zPath);
  unlink(zPath);
  if ((int)strlen(zPath) + 6 < (int)sizeof(buffer)) {
    sqlite3_snprintf(sizeof buffer, buffer, "%s-lock", zPath);
    unlink(buffer);
  }
}

#ifndef SQLITE_MUTEX_STATIC_TEMPDIR
# define SQLITE_MUTEX_STATIC_TEMPDIR SQLITE_MUTEX_STATIC_VFS1
#endif

/* Write a mkstemp() template for a temporary environment into zBuf, placed in
** the first usable directory of: sqlite3_temp_directory (PRAGMA
** temp_store_directory), $SQLITE_TMPDIR, $TMPDIR, /tmp, /var/tmp, /usr/tmp and
** ".".  These are the candidates SQLite's unix VFS uses for its own temporary
** files, read from the environment once, as SQLite 3.37 to 3.53 does, with
** /tmp moved ahead of /var/tmp so that the default location stays /tmp.  A
** directory is usable when it exists and is writable and searchable.  Returns
** 0 when none is. */
static int lumoTempTemplate(char *zBuf, int nBuf) {
  static const char *azEnv[2];
  static int envRead = 0;
  const char *azDir[7];
  struct stat st;
  int i, found = 0;
  sqlite3_mutex *mx = sqlite3_mutex_alloc(SQLITE_MUTEX_STATIC_TEMPDIR);
  sqlite3_mutex_enter(mx);
  if (!envRead) {
    azEnv[0] = getenv("SQLITE_TMPDIR");
    azEnv[1] = getenv("TMPDIR");
    envRead = 1;
  }
  azDir[0] = sqlite3_temp_directory;
  azDir[1] = azEnv[0];
  azDir[2] = azEnv[1];
  azDir[3] = "/tmp";
  azDir[4] = "/var/tmp";
  azDir[5] = "/usr/tmp";
  azDir[6] = ".";
  for (i = 0; i < 7 && !found; i++) {
    const char *z = azDir[i];
    if (z == 0 || z[0] == 0) continue;
    if ((int)strlen(z) + 20 > nBuf) continue;
    if (stat(z, &st) != 0 || !S_ISDIR(st.st_mode)) continue;
    if (access(z, W_OK | X_OK) != 0) continue;
    sqlite3_snprintf(nBuf, zBuf, "%s/dbml.etilqs.XXXXXX", z);
    found = 1;
  }
  sqlite3_mutex_leave(mx);
  return found;
}

/* Keep LMDB's file descriptors out of the stdio range (0, 1, 2).
**
** fossil server forks a child per connection and closes inherited descriptors
** before handling the request, which can leave fds 0/1/2 free.  LMDB then opens
** the repository data/lock/sync handles onto those low numbers, so the repo
** file ends up on fd 2; any later write to "stderr" (every fossil_errorlog()
** call, since g.zErrlog defaults to "-") goes write(2, ...) straight into the
** database and destroys it.  SQLite's os_unix.c guards against this
** (SQLITE_MINIMUM_FILE_DESCRIPTOR == 3); the LMDB backend must too.
**
** We pin any free descriptor in {0,1,2} before opening an env, forcing LMDB
** onto fd >= 3.  The placeholder is the current directory ".", which is usually
** openable, even inside fossil server's chroot jail, where /dev/null and /tmp
** are not reachable.  Repeated calls leave occupied descriptors unchanged. */
static void lumoReserveStdioFds(void) {
  int i, holder = -1;
  for (i = 0; i < 3; i++) {
    if (fcntl(i, F_GETFD) == -1 && errno == EBADF) {
      if (holder < 0) holder = open(".", O_RDONLY);
      if (holder < 0) return;
      if (holder == i) {
        holder = -1;
      } else if (dup2(holder, i) < 0) {
        return;
      }
    }
  }
  if (holder > 2) close(holder);
}

/* Open a database.  A file database is an LMDB environment in one file plus
** a "<name>-lock" file (MDB_NOSUBDIR).  A NULL or empty zFilename,
** ":memory:", or an open with SQLITE_OPEN_MEMORY creates a private LMDB
** environment at a mkstemp() path in the temporary directory, opened
** MDB_NOSYNC and removed with its lock file by sqlite3BtreeClose(). */
int sqlite3BtreeOpen(
  sqlite3_vfs *pVfs,      /* VFS to use for this b-tree */
  const char *zFilename,  /* Name of the file containing the BTree database */
  sqlite3 *db,            /* Associated database handle */
  Btree **ppBtree,        /* Pointer to new Btree object written here */
  int flags,              /* Options */
  int vfsFlags            /* Flags passed through to sqlite3_vfs.xOpen() */
){
  char dirPathName[BTREE_MAX_PATH];
  Btree *p = NULL;
  MDB_txn *txn = NULL;
  MDB_dbi dbi;
  int rc;
  int envClose = 0;
  int mdbFlags;

  /* True if opening an ephemeral, temporary database */
  const int isTempDb = zFilename==0 || zFilename[0]==0;

  const int isReadonly = (vfsFlags & SQLITE_OPEN_READONLY);

  /* Set the variable isMemdb to true for an in-memory database, or 
  ** false for a file-based database.
  */
#ifdef SQLITE_OMIT_MEMORYDB
  const int isMemdb = 0;
#else
  const int isMemdb = (zFilename && strcmp(zFilename, ":memory:")==0)
                       || (isTempDb && sqlite3TempInMemory(db))
                       || (vfsFlags & SQLITE_OPEN_MEMORY)!=0;
#endif

  LUMO_LOG("sqlite3BtreeOpen(%d, %d, %s)\n", isTempDb, isMemdb, zFilename ? zFilename : "?");
  mdbFlags = MDB_NOTLS;
  if (isReadonly) mdbFlags |= MDB_RDONLY;
  if (isMemdb || isTempDb) {
    /* The VFS unlinks a temporary file it creates as soon as it opens it, so
    ** LMDB could not reopen it by name, and mkstemp() creates the file outside
    ** the VFS.  The descriptor mkstemp() returns is closed and LMDB reopens
    ** the file with MDB_NOSUBDIR.  The file name contains "etilqs", "sqlite"
    ** spelled backwards, for the reason SQLite's os.h gives for its own
    ** temporary file prefix: a stray file stays identifiable without
    ** sending whoever finds it to the SQLite developers. */
    int tmpfd;
    mdbFlags |= MDB_NOSYNC | MDB_NOSUBDIR;
    if (!lumoTempTemplate(dirPathName, sizeof(dirPathName))) {
      LUMO_LOG("sqlite3BtreeOpen: no usable temporary directory\n");
      *ppBtree = 0;
      return SQLITE_IOERR_GETTEMPPATH;
    }
    tmpfd = mkstemp(dirPathName);
    if (tmpfd < 0) {
      /* Nothing is allocated yet and no file exists under this name, so
      ** there is nothing for the error path to undo or remove. */
      LUMO_LOG("sqlite3BtreeOpen: mkstemp failed\n");
      *ppBtree = 0;
      return SQLITE_CANTOPEN_BKPT;
    }
    close(tmpfd);
    LUMO_LOG("  file => %s\n", dirPathName);
  } else {
    sqlite3OsFullPathname(pVfs, zFilename, sizeof(dirPathName), dirPathName);
    /* LMDB stores the environment as one regular file plus a sibling
    ** "<name>-lock" file.  mdb_env_open() initialises an existing empty file,
    ** so a file created by the caller before the open is accepted. */
    mdbFlags |= MDB_NOSUBDIR;
  }
  p = sqlite3MallocZero(sizeof(Btree) + strlen(dirPathName) + 1);
  if (!p) {
    rc = ENOMEM;
    LUMO_LOG("sqlite3BtreeOpen: sqlite3MallocZero failed\n");
    goto error;
  }
  p->inTrans = SQLITE_TXN_NONE;
  p->svp = NULL;
  p->first_cursor = NULL;
  p->last_cursor = NULL;
  p->inBackup = 0;
  p->db = db;
  p->flags = flags;
  p->vfsFlags = vfsFlags;
  p->mdbFlags = mdbFlags;
  strcpy(p->path, dirPathName);
  p->isTemp = isMemdb || isTempDb;
  p->isMemdb = isMemdb;
  p->pSchema = NULL;
  p->xFreeSchema = NULL;
  lumoReserveStdioFds();   /* keep LMDB off fds 0/1/2 (see helper) */
  rc = mdb_env_create(&p->env);
  if (rc) goto error;
  envClose = 1;
  /* Map size: SSIZE_MAX/8 when SSIZE_MAX is below 64 TiB, otherwise 16 TiB,
  ** because the processor's virtual address space cannot be queried.
  ** Temporary databases opened with a NULL or empty filename map 32 MiB when
  ** SSIZE_MAX is below 64 TiB, otherwise 1 GiB.  LUMO_LMDB_MAPSIZE, in MiB,
  ** overrides the map size of every database, for containers and cgroup
  ** limits that count the reservation; a value below 4 or unparseable is
  ** ignored. */
  {
    size_t lumo_mapsize = 0;
    const char *lumo_mapsize_env = getenv("LUMO_LMDB_MAPSIZE");
    if (lumo_mapsize_env && *lumo_mapsize_env) {
      char *end = NULL;
      unsigned long long mb = strtoull(lumo_mapsize_env, &end, 10);
      if (end != lumo_mapsize_env && mb >= 4) {
        lumo_mapsize = (size_t)mb * (size_t)1024 * (size_t)1024;
      }
    }
    if (lumo_mapsize) {
      mdb_env_set_mapsize(p->env, lumo_mapsize);
    } else {
#define _terabyte (1024ULL * 1024ULL * 1024ULL * 1024ULL)
#if SSIZE_MAX < (64ULL * _terabyte)
      if (isTempDb)
        mdb_env_set_mapsize(p->env, (size_t)1024 * (size_t)1024 * (size_t)32);
      else
        mdb_env_set_mapsize(p->env, (size_t)SSIZE_MAX / 8);
#else
      if (isTempDb)
        mdb_env_set_mapsize(p->env, (size_t)1024 * (size_t)1024 * (size_t)1024);
      else
        mdb_env_set_mapsize(p->env, (size_t)_terabyte * 16);
#endif
#undef _terabyte
    }
  }
  mdb_env_set_maxreaders(p->env, 254);
  mdb_env_set_maxdbs(p->env, isTempDb ? 64 : 1024);

  /* LMDB opens the "-lock" file O_RDWR even for MDB_RDONLY to record this
  ** reader, so a lockfile without write permission returns EACCES.  MDB_NOLOCK
  ** is not a solution because such a reader is invisible to a concurrent writer,
  ** which may then recycle pages it is reading and so return wrong data with no error.
  ** A read-only filesystem is an exception where MDB_NOLOCK is safe. Otherwise, open
  ** MDB_NOLOCK only when the caller passes the nolock=1 URI parameter, which copies
  ** SQLite usage and means the application takes responsibility for excluding writers.
  ** In all other cases refuse the open. LMDB upstream refused read-only client mode,
  ** see https://bugs.openldap.org/show_bug.cgi?id=10562 */
  if ((mdbFlags & MDB_RDONLY) && !isTempDb && !isMemdb) {
    char *zLock = sqlite3_mprintf("%s-lock", dirPathName);
    if (zLock) {
      if (access(zLock, F_OK) == 0 && access(zLock, W_OK) != 0) {
        struct statvfs vfs;
        if (statvfs(dirPathName, &vfs) == 0 && (vfs.f_flag & ST_RDONLY)) {
          sqlite3_log(SQLITE_WARNING, "lumo-lmdb: \"%s\" is open without"
                      " reader locking on a read-only filesystem", dirPathName);
        } else if (sqlite3_uri_boolean(zFilename, "nolock", 0)) {
          sqlite3_log(SQLITE_WARNING, "lumo-lmdb: \"%s\" is open without"
                      " reader locking (nolock=1); a concurrent writer can"
                      " cause wrong results", dirPathName);
        } else {
          sqlite3_log(SQLITE_CANTOPEN, "lumo-lmdb: cannot open \"%s\""
                      " read-only: lock file \"%s\" is not writable; add"
                      " nolock=1 to the URI if no writer can run", dirPathName,
                      zLock);
          sqlite3_free(zLock);
          rc = SQLITE_CANTOPEN_BKPT;
          goto error_rc;
        }
        mdbFlags |= MDB_NOLOCK;
      }
      sqlite3_free(zLock);
    }
  }

  rc = mdb_env_open(p->env, dirPathName, mdbFlags, SQLITE_DEFAULT_FILE_PERMISSIONS);
  LUMO_LOG("sqlite3BtreeOpen: mdb_env_open (env %p) rc=%d maxkey=%d\n",
	   p->env, rc, mdb_env_get_maxkeysize(p->env));
  if (rc) goto error;
  /* Apply LumoSQL's default durability to file databases, whether or not
  ** LUMO_LMDB_SYNC is compiled in.  LumoSQL defaults to "normal"
  ** (MDB_NOMETASYNC: crash-safe, never corrupts, at most the last committed
  ** transaction lost).
  ** This must hold for every embedder, so it is applied even when the
  ** LUMO_LMDB_SYNC runtime-control option is not compiled in.  When that option
  ** is compiled in, LUMO_SYNC / -DLUMO_LMDB_SYNC_DEFAULT may override the level. */
  /* Temp and :memory: databases are discarded on close, so they keep the
  ** MDB_NOSYNC set at open: a durability level would clear it and fsync every
  ** commit on a database that never needs to survive a crash. */
  if (! (isTempDb || isMemdb)) {
#if defined(LUMO_LMDB_SYNC)
    const char *_lv = lumo_sync_open_level();   /* env / compile default / normal */
#else
    const char *_lv = "normal";                 /* LumoSQL default floor */
#endif
    if (_lv && *_lv) lumo_apply_sync(p, _lv);
  }
  /* Probe for "meta" in a read-only transaction, and begin a write
  ** transaction to create "t00000001" and "meta" only when "meta" is absent.
  ** Opening an existing database then does not start a write transaction,
  ** so with lmdb-lazy-dirty-list.mod the open does not allocate the write
  ** scratch. */
  rc = mdb_txn_begin(p->env, NULL, MDB_RDONLY, &txn);
  LUMO_LOG("mdb_txn_begin(RDONLY probe): %d\n", rc);
  if (rc) goto error;
  rc = mdb_dbi_open(txn, "meta", 0, &dbi);
  if (rc == 0) {
    p->transType = get_meta_32_direct(txn, dbi, LUMO_LMDB_TRANSACTION);
    mdb_txn_abort(txn);
    txn = NULL;
    if (p->transType != LMDB_TRANSACTION_OPTIMISTIC &&
        p->transType != LMDB_TRANSACTION_NOUPGRADE &&
        p->transType != LMDB_TRANSACTION_SERIALISE) {
      rc = SQLITE_CORRUPT_BKPT;
      goto error_rc;
    }
    *ppBtree = p;
    return SQLITE_OK;
  }
  mdb_txn_abort(txn);
  txn = NULL;
  if (rc != MDB_NOTFOUND) goto error;
  if (isReadonly) goto error;

  /* "meta" absent and read/write: create the sub-databases (once per db). */
  rc = mdb_txn_begin(p->env, NULL, 0, &txn);
  LUMO_LOG("mdb_txn_begin(create): %d\n", rc);
  if (rc) goto error;
  rc = mdb_dbi_open(txn, "t00000001", MDB_CREATE, &dbi);
  LUMO_LOG("mdb_dbi_open t00000001: %d\n", rc);
  if (rc) goto error;
  rc = mdb_dbi_open(txn, "meta", MDB_CREATE, &dbi);
  LUMO_LOG("mdb_dbi_open meta: %d\n", rc);
  if (rc) goto error;
  p->transType = get_meta_32_direct(txn, dbi, LUMO_LMDB_TRANSACTION);
  if (p->transType != LMDB_TRANSACTION_OPTIMISTIC &&
      p->transType != LMDB_TRANSACTION_NOUPGRADE &&
      p->transType != LMDB_TRANSACTION_SERIALISE) {
    rc = SQLITE_CORRUPT_BKPT;
    goto error_rc;
  }
  rc = mdb_txn_commit(txn);
  LUMO_LOG("mdb_dbi_open commit: %d\n", rc);
  if (rc) {
    txn = NULL;
    goto error;
  }
  *ppBtree = p;
  return SQLITE_OK;
  /* rc holds an errno or MDB_* code at error, and an SQLite result code at
  ** error_rc. */
error:
  rc = error_map(rc);
error_rc:
  if (txn) mdb_txn_abort(txn);
  if (envClose) {
    LUMO_LOG("closing env %p\n", p->env);
    mdb_env_close(p->env);
  }
  if (isTempDb || isMemdb)
    removeTempDb(dirPathName);
  if (p) sqlite3_free(p);
  *ppBtree = 0;
  return rc;
}

/*
** Close an open database and invalidate all cursors.
*/
int sqlite3BtreeClose(Btree *p) {
  LUMO_LOG("sqlite3BtreeClose %s (env %p)\n", p->path, p->env);
  if (p->svp) {
    LUMO_LOG("  aborting txn %p\n", p->svp->txn);
    closeAllSavepoints(p, -1, 0, SQLITE_INTERNAL);
    invalidateCursors(p, p->svp, SQLITE_INTERNAL, 0);
    if (p->svp->txn) mdb_txn_abort(p->svp->txn);
    sqlite3_free(p->svp);
  }
  lumoProcessWriteUnlock(p);
  if (p->parkedReader) { mdb_txn_abort(p->parkedReader); p->parkedReader = NULL; }
  if (p->pEph) ephTreeFree(p);
  if (p->isTemp && *p->path) {
    if (p->env) mdb_env_close(p->env);
    removeTempDb(p->path);
  } else {
    mdb_env_close(p->env);
  }
  if (p->pSchema) {
    if (p->xFreeSchema) p->xFreeSchema(p->pSchema);
    sqlite3_free(p->pSchema);
  }
  sqlite3_free(p);
  return SQLITE_OK;
}

int sqlite3BtreeSetCacheSize(Btree *p, int mxPage) {
  return SQLITE_OK;
}

/* sqlite3BtreeSetSpillSize: upstream sets the page-cache spill threshold
** and returns the larger of the threshold and the cache size, in pages;
** mxPage 0 only queries it.
** LMDB does not use SQLite's page cache, so there is no threshold: return 0,
** which PRAGMA cache_spill then reports. */
int sqlite3BtreeSetSpillSize(Btree *p, int mxPage) {
  LUMO_LOG("called: sqlite3BtreeSetSpillSize(%d)\n", mxPage);
  return 0;
}

/* sqlite3BtreeSetMmapLimit: in upstream, propagates SQLITE_DEFAULT_MMAP_SIZE
** (or PRAGMA mmap_size) into the pager.  LMDB always memory-maps and
** controls the mmap size itself at env_open via mdb_env_set_mapsize, so
** this is a no-op.  Upstream returns SQLITE_OK unconditionally. */
int sqlite3BtreeSetMmapLimit(Btree *p, sqlite3_int64 szMap) {
  LUMO_LOG("called: sqlite3BtreeSetMmapLimit(%lld)\n", (long long)szMap);
  return SQLITE_OK;
}

/* sqlite3BtreeSetPagerFlags: upstream passes the PAGER_* flags set by
** PRAGMA synchronous and related pragmas to the pager.  LMDB has no SQLite
** pager and lumo_apply_sync() sets durability, so the flags are ignored. */
int sqlite3BtreeSetPagerFlags(Btree *p, unsigned pgFlags) {
  LUMO_LOG("called: sqlite3BtreeSetPagerFlags(0x%x)\n", pgFlags);
  return SQLITE_OK;
}

/* sqlite3BtreeSetPageSize: LMDB's page size is fixed when the environment
** is created, so the request is ignored.  Return SQLITE_OK because VACUUM
** (vacuum.c) fails on any nonzero return, and reports a nonzero return for
** the temporary database as SQLITE_NOMEM. */
int sqlite3BtreeSetPageSize(Btree *p, int nPagesize, int nReserve, int eFix) {
  LUMO_LOG("called: sqlite3BtreeSetPageSize(%d, %d, %d)\n",
           nPagesize, nReserve, eFix);
  (void)p; (void)nPagesize; (void)nReserve; (void)eFix;
  return SQLITE_OK;
}

/* sqlite3BtreeGetPageSize: in upstream, returns the current page size
** in bytes.  Plenty of consumers (dbstat, backup, vacuum) pass the
** result around as "how many bytes per page", so we need a real number.
** LMDB's actual page size is queryable via mdb_env_stat -> ms_psize. */
int sqlite3BtreeGetPageSize(Btree *p) {
  MDB_stat st;
  int rc;
  if (p == NULL || p->env == NULL) return 4096;  /* sensible default */
  rc = mdb_env_stat(p->env, &st);
  if (rc != 0) {
    LUMO_LOG("sqlite3BtreeGetPageSize: mdb_env_stat -> %d\n", rc);
    return 4096;
  }
  return (int)st.ms_psize;
}

/* sqlite3BtreeIsInTrans / sqlite3BtreeIsInReadTrans were public Btree
** API entry points pre-3.34.  From 3.34 onward callers were migrated to
** sqlite3BtreeTxnState() and the Is* wrappers were removed.  Provide
** the two compat shims for pre-3.34 builds.  Both are one-liners over
** our internal Btree::inTrans field (which uses SQLITE_TXN_* sentinels
** defined as fallbacks at the top of this file when missing). */
#if SQLITE_VERSION_NUMBER < 3034000
int sqlite3BtreeIsInTrans(Btree *p) {
  return p && p->inTrans != SQLITE_TXN_NONE;
}
int sqlite3BtreeIsInReadTrans(Btree *p) {
  return p && p->inTrans == SQLITE_TXN_READ;
}
#endif

/* sqlite3BtreeMaxPageCount: upstream sets the page limit when mxPage is
** nonzero and returns the limit in force.  LMDB's limit is the map size, so
** mxPage is ignored and the largest page count is returned, which is what
** PRAGMA max_page_count reports. */
/* SQLite changed this from `int (Btree*, int)` to `Pgno (Btree*, Pgno)`
** between 3.32 and 3.33 (when Pgno was widened from int to u32
** throughout the btree API).  Pre-3.33 builds need the older signature. */
#if SQLITE_VERSION_NUMBER >= 3033000
Pgno sqlite3BtreeMaxPageCount(Btree *p, Pgno mxPage) {
  LUMO_LOG("called: sqlite3BtreeMaxPageCount(%u)\n", (unsigned)mxPage);
  /* the default SQLITE_MAX_PAGE_COUNT in SQLite 3.45 and later */
  return 0xFFFFFFFE;
}
#else
int sqlite3BtreeMaxPageCount(Btree *p, int mxPage) {
  LUMO_LOG("called: sqlite3BtreeMaxPageCount(%d)\n", mxPage);
  /* near INT_MAX, the largest value of the pre-3.33 int return type */
  return 0x7FFFFFFE;
}
#endif

/*
** Return the last page number LMDB has used or, while a transaction is open,
** the largest root page number if that is greater.  Return (Pgno)-1 if
** mdb_env_info() fails.
*/
Pgno sqlite3BtreeLastPage(Btree *p){
  MDB_envinfo info;
  u32 pgnoRoot;
  int rc = mdb_env_info(p->env, &info);
  if (rc) {
    LUMO_LOG("sqlite3BtreeLastPage: fail %d\n", rc);
    return (unsigned int)-1;
  }
  LUMO_LOG("sqlite3BtreeLastPage(%u)\n", (unsigned int)info.me_last_pgno);
  /* The schema loader (sqlite3InitCallback in prepare.c) treats a root page
  ** greater than the value this function returns as corrupt.  Root page
  ** numbers here are names of LMDB sub-databases, not pages, so report at
  ** least the largest one. */
  if (! p->svp) return info.me_last_pgno;
  sqlite3BtreeGetMeta(p, BTREE_LARGEST_ROOT_PAGE, &pgnoRoot);
  LUMO_LOG("sqlite3BtreeLastPage(%u, %u)\n", (unsigned int)info.me_last_pgno, pgnoRoot);
  return info.me_last_pgno < pgnoRoot ? pgnoRoot : info.me_last_pgno;
}

/* sqlite3BtreeSecureDelete: upstream sets whether deleted content is
** overwritten with zeros.  LMDB has no equivalent: freed pages keep their old
** contents until reused.  Report "off". */
int sqlite3BtreeSecureDelete(Btree *p, int newFlag) {
  LUMO_LOG("called: sqlite3BtreeSecureDelete(%d)\n", newFlag);
  return 0;
}

/* sqlite3BtreeGetRequestedReserve: in upstream, returns the maximum
** "reserved" bytes per page that any extension has asked for (used by
** codecs to leave room for nonces).  LMDB doesn't have per-page
** reserves.  Return 0. */
int sqlite3BtreeGetRequestedReserve(Btree *p) {
  LUMO_LOG("called: sqlite3BtreeGetRequestedReserve\n");
  return 0;
}

/* sqlite3BtreeSetAutoVacuum: in upstream, toggles autovacuum mode.
** LMDB's allocator handles freed pages on its own (no rebuild step),
** so SQLite-level autovacuum has no analogue.  Returning SQLITE_READONLY
** matches the SQLITE_OMIT_AUTOVACUUM build path upstream and signals
** "the value cannot be changed". */
int sqlite3BtreeSetAutoVacuum(Btree *p, int autoVacuum) {
  LUMO_LOG("called: sqlite3BtreeSetAutoVacuum(%d)\n", autoVacuum);
  return SQLITE_READONLY;
}

/* sqlite3BtreeGetAutoVacuum: returns BTREE_AUTOVACUUM_NONE/FULL/INCR.
** LMDB has no autovacuum, so always NONE. */
int sqlite3BtreeGetAutoVacuum(Btree *p) {
  LUMO_LOG("called: sqlite3BtreeGetAutoVacuum\n");
  return BTREE_AUTOVACUUM_NONE;
}

/*
** Start a read transaction, or a write transaction if wrFlag is nonzero;
** values of 2 or more are treated as 1.  A write transaction first takes the
** process write lock, invoking the busy handler while another connection
** in this process holds it.
** Upgrading an open read transaction follows the database's transaction
** model: "noupgrade" returns SQLITE_BUSY, "optimistic" replaces it with a
** write transaction and returns SQLITE_BUSY if another writer committed in
** between, and "serialise" already holds an LMDB write transaction.  If
** pSchemaVersion is not NULL, it receives the schema cookie.
*/
int sqlite3BtreeBeginTrans(Btree *p, int wrFlag, int *pSchemaVersion) {
  const int isReadonly = p->vfsFlags & SQLITE_OPEN_READONLY;
  int rc;
  LUMO_LOG("sqlite3BtreeBeginTrans(%d/%d, %d, %s) %s\n",
	   wrFlag, isReadonly, p->inTrans, LUMO_TXN(p->inTrans), p->path);
  /* sqlite3BtreeCopyFile leaves no env when it cannot reopen the database */
  if (p->env == NULL) return SQLITE_IOERR;
  /* if we already have a read/write transaction, there's nothing
  ** more to do.  But we still need to materialise any named savepoints
  ** that SQLite has BEGUN since our last call (see syncNamedSavepoints
  ** above for why this is here rather than in vdbe.c). */
  if (p->inTrans == SQLITE_TXN_WRITE) {
    if (wrFlag) {
      rc = syncNamedSavepoints(p);
      if (rc) return rc;
    }
    goto get_version;
  }
  if (wrFlag && isReadonly != 0)
    return SQLITE_READONLY;
  if (wrFlag) {
    do {
      rc = lumoProcessWriteLock(p);
    } while (rc == SQLITE_BUSY && sqlite3InvokeBusyHandler(&p->db->busyHandler));
    if (rc) return rc;
  }
  if (p->inTrans != SQLITE_TXN_NONE) {
    /* we already have a read-only transaction; if wrFlag is zero we
    ** don't need to do anything */
    if (! wrFlag) goto get_version;
    /* we need to upgrade a read-only transaction to read/write; how
    ** we do that depends on the transaction model requested */
    if (p->transType == LMDB_TRANSACTION_NOUPGRADE) {
      /* "noupgrade" - we cannot go from a read-only to a read/write
      ** transaction */
      lumoProcessWriteUnlock(p);
      return SQLITE_BUSY;
    }
    if (p->transType == LMDB_TRANSACTION_OPTIMISTIC) {
      /* "optimistic" - we try to go from read-only to read/write
      ** transaction by beginning a new read/write transaction and
      ** comparing the commit counter: if unchanged, we "copy" all
      ** cursors across and abort the read-only transaction */
      u64 uCounterRO = get_meta_64(p, LUMO_LMDB_META_COUNTER), uCounterRW;
      rc = invalidateCursors(p, p->svp, SQLITE_OK, 0);
      if (rc) {
	LUMO_LOG("sqlite3BtreeBeginTrans: invalidateCursors -> %d\n", rc);
	lumoProcessWriteUnlock(p);
	return error_map(rc);
      }
      /* we must abort the r/o transaction before beginning a new one */
      p->hasData = 0;
      mdb_txn_abort(p->svp->txn);
      /* Aborting the read transaction closed any dbi handle that get_table()
      ** first opened under it, but the dbi cache is keyed on p->svp, which has
      ** not changed.  Invalidate the cache so that the next get_table()
      ** reopens the dbi in the write transaction; a stale handle makes
      ** mdb_cursor_open() fail with EINVAL. */
      p->gtCacheValid = 0;
      rc = mdb_txn_begin(p->env, NULL, 0, &p->svp->txn);
      if (rc) goto out_error;
      rc = mdb_dbi_open(p->svp->txn, "meta", 0, &p->dataDbi);
      if (rc) {
	mdb_txn_abort(p->svp->txn);
      out_error:
	LUMO_LOG("sqlite3BtreeBeginTrans: upgrade fail %d\n", rc);
	sqlite3_free(p->svp);
	p->svp = NULL;
	p->inTrans = SQLITE_TXN_NONE;   /* the read transaction is already aborted */
	lumoProcessWriteUnlock(p);
	return error_map(rc);
      }
      p->hasData = 1;
      uCounterRW = get_meta_64(p, LUMO_LMDB_META_COUNTER);
      LUMO_LOG("uCounterRO=%llu  uCounterRW=%llu\n",
	       (unsigned long long)uCounterRO, (unsigned long long)uCounterRW);
      if (uCounterRO != uCounterRW) {
	/* Another writer committed after the read snapshot, and that snapshot
	** is already gone, so end the transaction: keeping the write
	** transaction open would block every other writer. */
	mdb_txn_abort(p->svp->txn);
	sqlite3_free(p->svp);
	p->svp = NULL;
	p->hasData = 0;
	p->inTrans = SQLITE_TXN_NONE;
	lumoProcessWriteUnlock(p);
	return SQLITE_BUSY;
      }
      p->inTrans = SQLITE_TXN_WRITE;
      goto get_version;
    }
    if (p->transType == LMDB_TRANSACTION_SERIALISE) {
      /* "serialise" all transactions by always using a read/write
      ** transaction.  Upgrading read-only to read/write only requires
      ** remembering that "commit" will now do real work */
      p->inTrans = SQLITE_TXN_WRITE;
      goto get_version;
    }
    /* invalid transaction value... */
    lumoProcessWriteUnlock(p);
    return SQLITE_CORRUPT_BKPT;
  }
  if (p->svp) {
    if (wrFlag) lumoProcessWriteUnlock(p);
    return SQLITE_INTERNAL;
  }
  p->gtCacheValid = 0;  /* new transaction: drop the cached dbi resolution */
  p->svp = sqlite3DbMallocZero(0, sizeof(BtreeSavepoint));
  if (! p->svp) {
    if (wrFlag) lumoProcessWriteUnlock(p);
    return SQLITE_NOMEM;
  }
  p->svp->prev = NULL;
  p->svp->nSavepoint = -1;
  if (p->transType != LMDB_TRANSACTION_SERIALISE) {
    /* "noupgrade" or "optimistic" - begin a read-only or read/write transaction
    ** as requested */
    if (!wrFlag && p->parkedReader) {
      /* revive the reader parked by the previous autocommit read: renew
      ** refreshes the snapshot to the latest committed txn, as a fresh
      ** begin would, but skips the reader-table slot allocation. */
      rc = mdb_txn_renew(p->parkedReader);
      if (rc == 0) { p->svp->txn = p->parkedReader; p->parkedReader = NULL; }
      else { mdb_txn_abort(p->parkedReader); p->parkedReader = NULL;
             rc = mdb_txn_begin(p->env, NULL, MDB_RDONLY, &p->svp->txn); }
    } else {
      if (wrFlag && p->parkedReader) {
        mdb_txn_abort(p->parkedReader); p->parkedReader = NULL;
      }
      rc = mdb_txn_begin(p->env, NULL, wrFlag ? 0 : MDB_RDONLY, &p->svp->txn);
    }
  } else {
    /* "serialise" - begin a read/write transaction unless the connection is
    ** read-only */
    if (p->parkedReader) { mdb_txn_abort(p->parkedReader); p->parkedReader = NULL; }
    rc = mdb_txn_begin(p->env, NULL, isReadonly ? MDB_RDONLY : 0, &p->svp->txn);
  }
  if (rc) {
    LUMO_LOG("sqlite3BtreeBeginTrans (%d): begin fail %d\n", wrFlag, rc);
    sqlite3_free(p->svp);
    p->svp = NULL;
    if (wrFlag) lumoProcessWriteUnlock(p);
    return error_map(rc);
  }
  rc = mdb_dbi_open(p->svp->txn, "meta", 0, &p->dataDbi);
  if (rc == MDB_NOTFOUND && !wrFlag) {
    p->hasData = 0;
  } else if (rc) {
    mdb_txn_abort(p->svp->txn);
    sqlite3_free(p->svp);
    p->svp = NULL;
    if (wrFlag) lumoProcessWriteUnlock(p);
    LUMO_LOG("sqlite3BtreeBeginTrans (%d): dbi open fail %d\n", wrFlag, rc);
    return error_map(rc);
  } else {
    p->hasData = 1;
  }
  p->inTrans = wrFlag ? SQLITE_TXN_WRITE : SQLITE_TXN_READ;
get_version:
  /* If SQLite has issued SAVEPOINT BEGIN(s) since we last looked, push
  ** matching nested LMDB txns now so subsequent writes land in the
  ** right place.  Skipped for read-only transactions since
  ** mdb_txn_begin with a parent always implies write. */
  if (wrFlag && p->inTrans == SQLITE_TXN_WRITE) {
    rc = syncNamedSavepoints(p);
    if (rc) return rc;
  }
  {
    u64 counter = get_meta_64(p, LUMO_LMDB_META_COUNTER);
    if (!p->dataVersionValid) {
      p->dataVersionCounter = counter;
      p->dataVersionValid = 1;
      p->iDataVersion = 1;
    } else if (counter != p->dataVersionCounter) {
      p->dataVersionCounter = counter;
      p->iDataVersion++;
    }
  }
  if (pSchemaVersion)
    *pSchemaVersion = get_meta_32(p, BTREE_SCHEMA_VERSION);
  LUMO_LOG("sqlite3BtreeBeginTrans(%d, %d) OK txn=%p %s\n",
	   wrFlag,
	   pSchemaVersion ? *pSchemaVersion : get_meta_32(p, BTREE_SCHEMA_VERSION),
	   p->svp->txn, p->path);
  return SQLITE_OK;
}

/*
** Commit the transaction.  LMDB commits atomically in one step, so this does
** all of the work: for a write transaction it increments the commit counter
** and commits, and it aborts a read transaction.  Every open cursor that
** holds an LMDB cursor is tripped with SQLITE_INTERNAL, so a statement still
** reading this database fails on its next step.  zSuperJrnl is ignored, so a
** transaction across several attached databases is not atomic across them.
*/
int sqlite3BtreeCommitPhaseOne(Btree *p, const char *zSuperJrnl) {
  LUMO_LOG("sqlite3BtreeCommitPhaseOne(%d, %s) txn=%p %s\n",
	   p->inTrans, LUMO_TXN(p->inTrans), p->svp ? p->svp->txn : NULL, p->path);
  if (p->inTrans != SQLITE_TXN_NONE) {
    int rc = closeAllSavepoints(p, -1, p->inTrans == SQLITE_TXN_WRITE, SQLITE_INTERNAL);
    if (rc) return error_map(rc);
    invalidateCursors(p, p->svp, SQLITE_INTERNAL, 0);
    if (p->inTrans == SQLITE_TXN_WRITE) {
      u64 uCounter = get_meta_64(p, LUMO_LMDB_META_COUNTER) + 1;
      rc = put_meta_64(p, LUMO_LMDB_META_COUNTER, uCounter);
      if (rc) {
	LUMO_LOG("sqlite3BtreeCommitPhaseOne: fail updating commit counter %p %d\n",
	         p->svp->txn, rc);
	mdb_txn_abort(p->svp->txn);
	sqlite3_free(p->svp);
	p->svp = NULL;
	p->inTrans = SQLITE_TXN_NONE;
	lumoProcessWriteUnlock(p);
	return error_map(rc);
      }
      rc = mdb_txn_commit(p->svp->txn);
      p->inTrans = SQLITE_TXN_NONE;
      lumoProcessWriteUnlock(p);
      if (rc) {
	LUMO_LOG("sqlite3BtreeCommitPhaseOne: fail %p %d\n", p->svp->txn, rc);
	/* mdb_txn_commit frees the transaction when it fails, too */
	sqlite3_free(p->svp);
	p->svp = NULL;
	return error_map(rc);
      }
      p->dataVersionCounter = uCounter;
      p->dataVersionValid = 1;
      p->nOwnCommit++;
      LUMO_LOG("sqlite3BtreeCommitPhaseOne: OK %p, commit counter=%lld\n",
	       p->svp->txn, (long long)uCounter);
    } else {
      mdb_txn_abort(p->svp->txn);
      LUMO_LOG("sqlite3BtreeCommitPhaseOne: abort r/o transaction %p\n", p->svp->txn);
      p->inTrans = SQLITE_TXN_NONE;
    }
    sqlite3_free(p->svp);
    p->svp = NULL;
  }
  return SQLITE_OK;
}

int sqlite3BtreeCommitPhaseTwo(Btree *p, int bCleanup) {
  /* SQLite's autocommit cleanup calls CommitPhaseTwo on every attached
  ** database whose transaction state is not NONE, including pure readers
  ** (see the commit loop in vdbeCommit / sqlite3VdbeHalt).  A write
  ** transaction is already fully committed in CommitPhaseOne (LMDB has no
  ** real two-phase commit), so inTrans is NONE for writers by now.  A
  ** read-only transaction, however, may still be open as a lingering
  ** snapshot.  End it here so sqlite3BtreeTxnState() reports NONE.  DETACH
  ** refuses a database whose state is not NONE.
  **
  ** Only end the read transaction when no cursors remain open on this Btree.
  ** CommitPhaseTwo is also reached on intermediate transaction boundaries
  ** where SQLite still holds open cursors against the read snapshot and
  ** expects it to stay valid.  Tearing the txn down there would orphan
  ** those cursors (p->svp->txn becomes NULL) and surface as
  ** SQLITE_CANTOPEN on the next cursor operation. */
  if (p->inTrans == SQLITE_TXN_READ && p->svp && p->first_cursor == NULL) {
    closeAllSavepoints(p, -1, 0, SQLITE_OK);
    if (p->svp) {
      /* No cursors remain, so nothing references the read txn.  Park it with
      ** reset (keeps its reader-table slot) for the next read to renew, rather
      ** than aborting; renew is far cheaper than a fresh mdb_txn_begin. */
      if (p->svp->txn && !p->parkedReader) {
        mdb_txn_reset(p->svp->txn);
        p->parkedReader = p->svp->txn;
      } else if (p->svp->txn) {
        mdb_txn_abort(p->svp->txn);
      }
      sqlite3_free(p->svp);
      p->svp = NULL;
    }
    p->inTrans = SQLITE_TXN_NONE;
    LUMO_LOG("sqlite3BtreeCommitPhaseTwo: ended idle r/o transaction (%s)\n", p->path);
  }
  return SQLITE_OK;
}

int sqlite3BtreeCommit(Btree *p) {
  return sqlite3BtreeCommitPhaseOne(p, NULL);
}

/*
** Rollback the transaction in progress.
**
** If tripCode is not SQLITE_OK, cursors are tripped and any later use
** returns tripCode.  With writeOnly true only write cursors are tripped,
** unless a savepoint or statement transaction is open, in which case every
** cursor is tripped.
**
** The LMDB transaction is aborted and the process write lock released,
** whether or not cursors remain open.
*/
int sqlite3BtreeRollback(Btree *p, int tripCode, int writeOnly) {
  if (p->inTrans != SQLITE_TXN_NONE) {
    LUMO_LOG("sqlite3BtreeRollback %s abort %p\n", p->path, p->svp->txn);
    closeAllSavepoints(p, -1, 0, tripCode);
    invalidateCursors(p, p->svp, tripCode, writeOnly);
    mdb_txn_abort(p->svp->txn);
    p->inTrans = SQLITE_TXN_NONE;
    lumoProcessWriteUnlock(p);
    sqlite3_free(p->svp);
    p->svp = NULL;
  }
  LUMO_LOG("sqlite3BtreeRollback done\n");
  return SQLITE_OK;
}

/*
** Start a statement subtransaction.  SQLite opens one around a statement
** inside a BEGIN...COMMIT block, so that a constraint error rolls back that
** statement and leaves the rest of the transaction.  This code begins a
** nested LMDB transaction under the innermost open savepoint and pushes it on
** the savepoint stack with number iStatement, the count of savepoints open
** including this one.  sqlite3BtreeSavepoint() releases or rolls it back.
*/
int sqlite3BtreeBeginStmt(Btree *p, int iStatement) {
  int rc;
  BtreeSavepoint * S = sqlite3DbMallocZero(0, sizeof(BtreeSavepoint));
  if (! S) return SQLITE_NOMEM;
  rc = mdb_txn_begin(p->env, p->svp ? p->svp->txn : NULL, 0, &S->txn);
  if (rc) {
    sqlite3_free(S);
    LUMO_LOG("sqlite3BtreeBeginStmt(%d): %p fail %d\n",
	     iStatement, p->svp ? p->svp->txn : NULL, rc);
    return error_map(rc);
  }
  LUMO_LOG("sqlite3BtreeBeginStmt(%d): %p OK %p\n",
	   iStatement, p->svp ? p->svp->txn : NULL, S->txn);
  S->nSavepoint = iStatement;
  S->prev = p->svp;
  p->svp = S;
  return SQLITE_OK;
}

/*
** Create a table and write its root page number to *piTable.  The number is
** one more than BTREE_LARGEST_ROOT_PAGE in "meta", at least 3, and
** identifies the LMDB sub-database that get_table() creates.  flags
** containing BTREE_INTKEY makes a rowid table; otherwise the table is an
** index and uses index_compare, or index_compare_temp on a BTREE_SINGLE
** btree.
*/
int sqlite3BtreeCreateTable(Btree *p, Pgno *piTable, int flags) {
  MDB_dbi dbi;
  u32 pgnoRoot;
  int rc;
  sqlite3BtreeGetMeta(p, BTREE_LARGEST_ROOT_PAGE, &pgnoRoot);
  if (pgnoRoot < 2)
    pgnoRoot = 3;
  else
    pgnoRoot++;
  *piTable = pgnoRoot;
  rc = get_table(p, pgnoRoot,
		 get_table_create | ((flags & BTREE_INTKEY) ? 0 : get_table_index),
		 &dbi);
  LUMO_LOG("sqlite3BtreeCreateTable(%u): %d\n", pgnoRoot, rc);
  if (rc) return rc;
  return sqlite3BtreeUpdateMeta(p, BTREE_LARGEST_ROOT_PAGE, pgnoRoot);
}

int sqlite3BtreeTxnState(Btree *p) {
  return p ? p->inTrans : SQLITE_TXN_NONE;
}

int sqlite3BtreeIsInBackup(Btree *p) {
  return p->inBackup;
}

/*
** This function returns a pointer to a blob of memory associated with
** a single Btree. The memory is used by client code for its own
** purposes (for example, to store a high-level schema associated with
** the Btree).
**
** The first time this is called on a Btree, nBytes bytes of memory
** are allocated, zeroed, and returned to the caller. For each subsequent 
** call the nBytes parameter is ignored and a pointer to the same blob
** of memory returned. 
**
** If the nBytes parameter is 0 and the blob of memory has not yet been
** allocated, a null pointer is returned. If the blob has already been
** allocated, it is returned as normal.
**
** Just before the Btree is closed, the function passed as the 
** xFree argument when the memory allocation was made is invoked on the 
** blob of allocated memory. The xFree function should not call sqlite3_free()
** on the memory, the btree layer does that.
*/
void *sqlite3BtreeSchema(Btree *p, int nBytes, void(*xFree)(void *)){
  if (nBytes > 0 && !p->pSchema) {
    p->pSchema = sqlite3DbMallocZero(0, nBytes);
    p->xFreeSchema = xFree;
  }
  return p->pSchema;
}

int sqlite3BtreeSchemaLocked(Btree *p) {
  return SQLITE_OK;
}

int sqlite3BtreeLockTable(Btree *p, int iTab, u8 isWriteLock) {
  return SQLITE_OK;
}

/*
** Release (op == SAVEPOINT_RELEASE) or roll back (op == SAVEPOINT_ROLLBACK)
** savepoint iSavepoint, each held as a nested LMDB transaction.  Rolling
** back leaves the savepoint open with a fresh nested transaction.  With
** iSavepoint == -1, SAVEPOINT_ROLLBACK aborts and ends the whole
** transaction, unlike upstream, which keeps it open.
*/
int sqlite3BtreeSavepoint(Btree *p, int op, int iSavepoint) {
  BtreeSavepoint * S;
  int rc;
  /* SQLite calls this on every entry in db->aDb[] during OP_Savepoint.
   * Slots that aren't active (e.g. the temp db when no temp table exists)
   * have a NULL pBt, so this no-ops on NULL.  ROLLBACK TO savepoint walks
   * every slot, including the inactive ones. */
  if (p == NULL) return SQLITE_OK;

  /* Walk the savepoint stack down to the target.  closeAllSavepoints
  ** stops with the target itself still on top of the stack (its loop
  ** condition is "stop when top->nSavepoint == nTo").  For ROLLBACK
  ** that's what we want.  For RELEASE we still need to fold the target
  ** itself into its parent below.
  **
  ** For ROLLBACK we use commit=0 so each unwound nested txn is aborted.
  ** For RELEASE we use commit=1: nested txns above the target get
  ** committed into their parent.  The target itself is then handled
  ** explicitly below. */
  rc = closeAllSavepoints(p, iSavepoint, op == SAVEPOINT_RELEASE, SQLITE_OK);
  LUMO_LOG("sqlite3BtreeSavepoint op=%s iSavepoint=%d -> %d (%s)\n",
	   op == SAVEPOINT_RELEASE ? "RELEASE" : "ROLLBACK",
	   iSavepoint, rc, p->path);
  if (rc) return error_map(rc);

  if (iSavepoint >= 0) {
    /* Named-savepoint case.
    **
    ** If the named savepoint was BEGUN by SQLite but our lazy
    ** materialisation in syncNamedSavepoints never had a chance to
    ** push it (e.g. SAVEPOINT s1; ROLLBACK TO s1.  With no statement
    ** in between), there's nothing on our stack to act on - that's
    ** fine, no writes happened in the savepoint either, so a no-op
    ** return is the correct behaviour. */
    if (! p->svp || p->svp->nSavepoint != iSavepoint) {
      LUMO_LOG("sqlite3BtreeSavepoint: target depth %d not materialised, "
	       "no writes to %s\n", iSavepoint,
	       op == SAVEPOINT_RELEASE ? "release" : "roll back");
      return SQLITE_OK;
    }
    S = p->svp;
    if (op == SAVEPOINT_RELEASE) {
      /* RELEASE the target: commit its nested txn into the parent. */
      rc = invalidateCursors(p, S->prev, SQLITE_OK, 0);
      if (rc) return error_map(rc);
      rc = mdb_txn_commit(S->txn);
      if (rc) {
	LUMO_LOG("sqlite3BtreeSavepoint: RELEASE %d commit -> %d\n",
		 iSavepoint, rc);
	/* The nested txn is now invalid regardless. */
	p->svp = S->prev;
	sqlite3_free(S);
	return error_map(rc);
      }
      p->svp = S->prev;
      sqlite3_free(S);
      LUMO_LOG("sqlite3BtreeSavepoint: RELEASE %d done; new top=%p\n",
	       iSavepoint, p->svp ? p->svp->txn : NULL);
      return SQLITE_OK;
    }
    /* ROLLBACK TO the target: abort its nested txn, then immediately
    ** open a fresh nested txn at the same depth so subsequent writes
    ** within the (still-open) savepoint have somewhere to go.  Every cursor
    ** on this Btree that holds an open LMDB cursor, including those opened
    ** in enclosing transactions, is tripped with SQLITE_ABORT_ROLLBACK; a
    ** cursor whose position was already saved keeps it. */
    rc = invalidateCursors(p, S->prev, SQLITE_ABORT_ROLLBACK, 0);
    if (rc) return error_map(rc);
    mdb_txn_abort(S->txn);
    rc = mdb_txn_begin(p->env,
		       S->prev ? S->prev->txn : NULL,
		       0, &S->txn);
    if (rc) {
      LUMO_LOG("sqlite3BtreeSavepoint: ROLLBACK %d reopen -> %d\n",
	       iSavepoint, rc);
      /* On failure, drop the entry entirely; future writes will trigger
      ** a fresh push via syncNamedSavepoints. */
      p->svp = S->prev;
      sqlite3_free(S);
      return error_map(rc);
    }
    LUMO_LOG("sqlite3BtreeSavepoint: ROLLBACK %d done; reopened txn=%p\n",
	     iSavepoint, S->txn);
    return SQLITE_OK;
  }

  assert(op == SAVEPOINT_ROLLBACK);
  /* iSavepoint == -1: abort and end the whole transaction. */
  LUMO_LOG("sqlite3BtreeSavepoint: aborting full transaction\n");
  S = p->svp;
  LUMO_LOG("aborting txn=%p\n", S->txn);
  invalidateCursors(p, S, SQLITE_OK, 0);
  mdb_txn_abort(S->txn);
  p->svp = S->prev;
  sqlite3_free(S);
  p->inTrans = SQLITE_TXN_NONE;
  lumoProcessWriteUnlock(p);
  return SQLITE_OK;
}

/* LMDB has no WAL, so leave the output counters at the -1 values set by
** sqlite3_wal_checkpoint_v2() for a database outside WAL mode. */
int sqlite3BtreeCheckpoint(Btree *p, int eMode, int *pnLog, int *pnCkpt) {
  (void)p;
  (void)pnLog;
  (void)pnCkpt;
  LUMO_LOG("called: sqlite3BtreeCheckpoint(%d)\n", eMode);
  return SQLITE_OK;
}

/*
** Return the full pathname of the underlying database file.  Return
** an empty string if the database is in-memory or a TEMP database.
*/
const char *sqlite3BtreeGetFilename(Btree *p) {
  return p->isTemp ? "" : p->path;
}

/*
** LMDB has no journal file, so return NULL.  vdbeCommit() skips a database
** whose journal name is NULL when it writes a super-journal.
*/
const char *sqlite3BtreeGetJournalname(Btree *p) {
  LUMO_LOG("called: sqlite3BtreeGetJournalname\n");
  return NULL;
}

/* Rename the finished VACUUM copy zTmp over the database file zPath and
** remove its lock file, which mdb_env_open recreates.  Returns an SQLite
** result code; on failure zPath is unchanged. */
static int lumoVacuumReplace(const char *zTmp, const char *zPath) {
  char fn[BTREE_MAX_PATH + 8];
  if (rename(zTmp, zPath) != 0) return SQLITE_IOERR;
  sqlite3_snprintf(sizeof fn, fn, "%s-lock", zPath);
  unlink(fn);
  return SQLITE_OK;
}

int sqlite3BtreeCopyFile(Btree *pTo, Btree *pFrom) {
  int rc;
  int copyRc;               /* result of writing and renaming the copy */
  char tmp[BTREE_MAX_PATH + 8];
  MDB_envinfo toInfo;
  size_t toMapsize = 0;
  LUMO_LOG("called: sqlite3BtreeCopyFile (%s <- %s)\n", pTo->path, pFrom->path);

  /* SQLite's page-by-page CopyFile model (stream source pages into the
  ** already-open destination Btree) has no LMDB analogue: mdb_env_copy
  ** snapshots a whole env to a path in one consistent operation.  It cannot
  ** write into a destination env that is currently open with a live write
  ** transaction.  So we snapshot pFrom's env into a file beside pTo's, rename
  ** it over pTo's on-disk file only once the copy is complete, then reopen pTo
  ** and re-establish its base write transaction
  ** so the caller's pending COMMIT sees a consistent, populated env.  A
  ** single mdb_env_copy copies everything.
  **
  ** VACUUM is the only caller: pFrom is the rebuilt temp database and pTo the
  ** live database.  sqlite3_backup has its own implementation in backup.c, and
  ** VACUUM INTO writes its attached output directly.
  **
  ** Critically, pFrom arrives with an OPEN WRITE TRANSACTION (VACUUM has
  ** been writing the rebuilt schema/rows into it and has not committed
  ** yet).  mdb_env_copy needs to open its own internal read transaction on
  ** pFrom for a consistent snapshot, and on the same thread that holds an
  ** open write txn this DEADLOCKS.  So we commit pFrom's transaction here
  ** (which makes its data durable and releases the writer) via
  ** sqlite3BtreeCommitPhaseOne, which frees pFrom->svp and sets
  ** pFrom->inTrans = SQLITE_TXN_NONE.  VACUUM's subsequent
  ** sqlite3BtreeCommit(pFrom) is then a guarded no-op (its body runs only
  ** when inTrans != NONE), so there is no double-commit. */
  /* Both temp and in-memory ("isMemdb") databases are backed by a real
  ** on-disk LMDB environment in this shim: sqlite3BtreeOpen gives every
  ** isTempDb/isMemdb btree a mkstemp() single-file
  ** env (see sqlite3BtreeOpen above).  mdb_env_copy therefore has the
  ** real on-disk source env at pFrom->path and a real target file at
  ** pTo->path it needs, whether or not either side is flagged isMemdb.  VACUUM
  ** of an in-memory database (pTo->isMemdb) and VACUUM whose scratch source is
  ** a temp/in-memory database (pFrom->isMemdb, the usual case) both work.  No
  ** special-casing is required here. */
  /* 0. commit pFrom, so that mdb_env_copy can open its read transaction */
  if (pFrom->inTrans != SQLITE_TXN_NONE) {
    rc = sqlite3BtreeCommitPhaseOne(pFrom, 0);
    if (rc) return rc;
  }

  /* remember pTo's mapsize so the reopened env keeps the same cap */
  if (mdb_env_info(pTo->env, &toInfo) == 0) toMapsize = (size_t)toInfo.me_mapsize;

  /* 1. tear down pTo's open transaction(s) and env. */
  rc = closeAllSavepoints(pTo, -1, 0, SQLITE_OK);   /* roll back named svps */
  (void)rc;
  invalidateCursors(pTo, pTo->svp, SQLITE_ABORT, 0);
  if (pTo->svp) {
    if (pTo->svp->txn) mdb_txn_abort(pTo->svp->txn);
    sqlite3_free(pTo->svp);
    pTo->svp = NULL;
  }
  pTo->inTrans = SQLITE_TXN_NONE;
  pTo->hasData = 0;
  mdb_env_close(pTo->env);
  pTo->env = NULL;
  /* The copy is written to "<name>-vacuum" and renamed over pTo's file only
  ** once complete, so a failed copy leaves the database as it was.
  ** mdb_env_copy will not overwrite an existing file, so remove any left over
  ** from an earlier failure. */
  sqlite3_snprintf(sizeof tmp, tmp, "%s-vacuum", pTo->path);
  unlink(tmp);

  /* 2. snapshot the source env beside pTo's file.  mdb_env_copy begins its
  ** own read transaction. */
  rc = mdb_env_copy(pFrom->env, tmp);
  LUMO_LOG("sqlite3BtreeCopyFile: mdb_env_copy -> %d (%s)\n", rc, mdb_strerror(rc));
  copyRc = rc == 0 ? lumoVacuumReplace(tmp, pTo->path) : error_map(rc);
  if (copyRc != SQLITE_OK) unlink(tmp);

  /* 3. reopen pTo->env, which holds the copy or, if the copy failed, the
  ** database as it was, and re-establish a base write transaction, mirroring
  ** sqlite3BtreeBeginTrans(wrFlag=1). */
  rc = mdb_env_create(&pTo->env);
  if (rc) return error_map(rc);
  mdb_env_set_maxdbs(pTo->env, 1024);
  if (toMapsize) mdb_env_set_mapsize(pTo->env, toMapsize);
  mdb_env_set_maxreaders(pTo->env, 254);
  rc = mdb_env_open(pTo->env, pTo->path, pTo->mdbFlags, SQLITE_DEFAULT_FILE_PERMISSIONS);
  if (rc) { mdb_env_close(pTo->env); pTo->env = NULL; return error_map(rc); }

  pTo->svp = sqlite3DbMallocZero(0, sizeof(BtreeSavepoint));
  if (! pTo->svp) return SQLITE_NOMEM;
  pTo->svp->prev = NULL;
  pTo->svp->nSavepoint = -1;
  rc = mdb_txn_begin(pTo->env, NULL, 0, &pTo->svp->txn);
  if (rc) { sqlite3_free(pTo->svp); pTo->svp = NULL; return error_map(rc); }
  rc = mdb_dbi_open(pTo->svp->txn, "meta", 0, &pTo->dataDbi);
  if (rc) { mdb_txn_abort(pTo->svp->txn); sqlite3_free(pTo->svp); pTo->svp = NULL; return error_map(rc); }
  pTo->hasData = 1;
  pTo->inTrans = SQLITE_TXN_WRITE;
  return copyRc;
}

/* sqlite3BtreeIncrVacuum: in upstream, runs one step of incremental
** vacuum (consuming free pages from the freelist).  LMDB's allocator
** reuses freed pages automatically and there's no equivalent of an
** SQLite freelist to walk.  Returning SQLITE_DONE means "no work
** remaining"; that's the upstream sentinel that PRAGMA incremental_vacuum
** uses to stop iterating. */
int sqlite3BtreeIncrVacuum(Btree *p) {
  LUMO_LOG("called: sqlite3BtreeIncrVacuum\n");
  return SQLITE_DONE;
}

/*
** Drop table iTable by deleting its LMDB sub-database, or empty it for the
** schema table (iTable 1).  Root numbers never move, so *piMoved is 0.  Open
** cursors on the table are not checked.
*/
int sqlite3BtreeDropTable(Btree *p, int iTable, int *piMoved) {
  MDB_dbi dbi;
  int rc = get_table(p, iTable, get_table_nocmp, &dbi);
  LUMO_LOG("sqlite3BtreeDropTable: get_table=%d\n", rc);
  *piMoved = 0;
  if (rc) return rc;
  rc = mdb_drop(p->svp->txn, dbi, iTable > 1);
  if (rc) return error_map(rc);
  return SQLITE_OK;
}

/*
** Delete every entry of table iTable, keeping the table.  A btree with an
** in-memory EphTree is cleared in memory, whatever iTable is; otherwise the
** LMDB sub-database is emptied with mdb_drop().  If pnChange is not NULL,
** the number of entries removed is added to *pnChange.  Blob handles on the
** table are invalidated; other open cursors on the table are not checked.
*/
#if SQLITE_VERSION_NUMBER < 3037000
int sqlite3BtreeClearTable(Btree *p, int iTable, int *pnChange) {
  int entries = 0;
#else
int sqlite3BtreeClearTable(Btree *p, int iTable, i64 *pnChange) {
  i64 entries = 0;
#endif
  MDB_dbi dbi;
  int rc;
  if (p->pEph) {
    EphTree *t = (EphTree *)p->pEph;
    if (pnChange) *pnChange += t->nEntry;
    ephClear(t);
    return SQLITE_OK;
  }
  if (p->hasIncrblobCur) invalidateIncrblobCursors(p, (Pgno)iTable, 0, 1);
  rc = get_table(p, iTable, get_table_nocmp, &dbi);
  LUMO_LOG("sqlite3BtreeClearTable: get_table=%d\n", rc);
  if (rc) return rc;
  if (pnChange) {
    MDB_stat stat;
    rc = mdb_stat(p->svp->txn, dbi, &stat);
    if (rc == 0) entries = stat.ms_entries;
  }
  rc = mdb_drop(p->svp->txn, dbi, 0);
  if (rc) return error_map(rc);
  if (pnChange) *pnChange += entries;
  return SQLITE_OK;
}

int sqlite3BtreeClearTableOfCursor(BtCursor *pCur) {
  /* Delete every row of the ephemeral table pCur is open on.  OP_ResetSorter
  ** calls this for an ephemeral table that is not a sorter. */
  LUMO_LOG("called: sqlite3BtreeClearTableOfCursor (root=%u)\n", pCur->rootPage);
  return sqlite3BtreeClearTable(pCur->pBtree, pCur->rootPage, 0);
}

int sqlite3BtreeTripAllCursors(Btree *p, int errCode, int writeOnly) {
  /* With a transaction open, trip the write cursors on p with errCode when
  ** writeOnly is true, or every cursor when it is false.  OP_Savepoint calls
  ** this for a savepoint rollback, before sqlite3BtreeSavepoint(). */
  LUMO_LOG("called: sqlite3BtreeTripAllCursors (errCode=%d, writeOnly=%d)\n",
           errCode, writeOnly);
  if (!p || !p->svp) return SQLITE_OK;
  return invalidateCursors(p, p->svp, errCode, writeOnly);
}

/*
** Read meta value idx from the "meta" sub-database of the open transaction,
** or 0 if it is unset.  BTREE_DATA_VERSION is not stored: it is
** iDataVersion, which sqlite3BtreeBeginTrans() increments when the commit
** counter has changed.
*/
void sqlite3BtreeGetMeta(Btree *p, int idx, u32 *pValue) {
  if (idx == BTREE_DATA_VERSION)
    *pValue = p->iDataVersion;
  else
    *pValue = get_meta_32(p, idx);
  LUMO_LOG("sqlite3BtreeGetMeta(%d: %d)\n", idx, *pValue);
}

/* Write meta value idx to the "meta" sub-database in the open write transaction. */
int sqlite3BtreeUpdateMeta(Btree *p, int idx, u32 iValue) {
  int rc = put_meta_32(p, idx, iValue);
  if (rc) {
    LUMO_LOG("sqlite3BtreeUpdateMeta(%d): fail %d\n", idx, rc);
    return error_map(rc);
  }
  LUMO_LOG("sqlite3BtreeUpdateMeta(%d: %d)\n", idx, (int)iValue);
  return SQLITE_OK;
}

/*
** Open a cursor on table iTable in the current transaction, which the
** caller must have begun.  pKeyInfo is NULL for a rowid table and non-NULL
** for an index.  A BTREE_SINGLE (ephemeral) btree uses the in-memory EphTree
** unless the LUMO_NOEPH environment variable is set.  BTREE_FORDELETE is
** ignored.  The EphTree ignores iTable; otherwise the cursor fails to open
** if iTable has no sub-database.  pCur must have been zeroed by
** sqlite3BtreeCursorZero().
*/
int sqlite3BtreeCursor(
  Btree *p,                                   /* The btree */
  Pgno iTable,                                /* Root page of table to open */
  int wrFlag,                                 /* 1 to write. 0 read-only */
  struct KeyInfo *pKeyInfo,                   /* First arg to xCompare() */
  BtCursor *pCur                              /* Write new cursor here */
){
  int rc;
  LUMO_LOG("sqlite3BtreeCursor(p=%p pCur=%p iTable=%u wrflag=%d info=%s\n",
	   p, pCur, (unsigned int)iTable, wrFlag, pKeyInfo ? "yes" : "no");
  if (pCur->prev || pCur->next) {
    LUMO_LOG("sqlite3BtreeCursor: %p prev=%p next=%p\n", pCur, pCur->prev, pCur->next);
    return SQLITE_INTERNAL;
  }
  pCur->pBtree = p;
  pCur->pKeyInfo = pKeyInfo;
  pCur->rootPage = iTable;
  pCur->txn = p->svp->txn;
  pCur->cursor = NULL;
  if ((p->flags & BTREE_SINGLE) && getenv("LUMO_NOEPH") == NULL) {
    rc = ephCursorOpen(p, pCur, pKeyInfo);
    if (rc != SQLITE_OK) return rc;
    pCur->atEof = 0;
    pCur->prev = p->last_cursor;
    pCur->next = NULL;
    if (p->last_cursor) p->last_cursor->next = pCur; else p->first_cursor = pCur;
    p->last_cursor = pCur;
    pCur->wrFlag = wrFlag ? 1 : 0;
    return SQLITE_OK;
  }
  rc = get_table(p, iTable, pKeyInfo ? get_table_index : 0, &pCur->dbi);
  if (rc) {
    LUMO_LOG("sqlite3BtreeCursor(%s, %u): table fail %d\n", p->path, iTable, rc);
    return rc;
  }
  rc = mdb_cursor_open(pCur->txn, pCur->dbi, &pCur->cursor);
  if (rc) {
    LUMO_LOG("sqlite3BtreeCursor (%s, %u): open fail %d\n", p->path, iTable, rc);
    return error_map(rc);
  }
  /* SQLite positions a new cursor before reading from it */
  pCur->atEof = 1;
  pCur->cleared = 1;
  LUMO_LOG("sqlite3BtreeCursor(%s, %u) txn=%p cur=%p %p\n",
	   p->path, iTable, pCur->txn, pCur, pCur->cursor);
  pCur->prev = p->last_cursor;
  pCur->next = NULL;
  if (p->last_cursor)
    p->last_cursor->next = pCur;
  else
    p->first_cursor = pCur;
  p->last_cursor = pCur;
  pCur->wrFlag = wrFlag ? 1 : 0;
  return SQLITE_OK;
}

BtCursor *sqlite3BtreeFakeValidCursor(void) {
  LUMO_LOG("sqlite3BtreeFakeValidCursor\n");
  return &fakeCursor;
}

/*
** Return the size of a BtCursor object in bytes, rounded up to a multiple of
** 8.  The VDBE allocates its cursors with this size, because BtCursor is
** opaque outside this file.
*/
int sqlite3BtreeCursorSize(void){
  return ROUND8(sizeof(BtCursor));
}

/* Zero the memory that will hold a BtCursor. */
void sqlite3BtreeCursorZero(BtCursor *p){
  LUMO_LOG("sqlite3BtreeCursorZero(%p)\n", p);
  memset(p, 0, sizeof(BtCursor));
  p->tripCode = SQLITE_OK;
}

/*
** Provide flag hints to the cursor.
*/
void sqlite3BtreeCursorHintFlags(BtCursor *pCur, unsigned x) {
  pCur->hints = x;
}

/*
** Close a cursor and unlink it from its Btree.  Closing the last cursor on a
** BTREE_SINGLE btree also closes the btree.
*/
int sqlite3BtreeCloseCursor(BtCursor *pCur) {
  Btree *p;
  if (pCur->cursor) {
    LUMO_LOG("sqlite3BtreeCloseCursor %s, %u txn=%p cur=%p %p\n",
	     pCur->pBtree->path, pCur->rootPage,
	     pCur->pBtree->svp->txn, pCur, pCur->cursor);
    mdb_cursor_close(pCur->cursor);
    pCur->cursor = NULL;
  }
  if (pCur->savedKey.mv_data) {
    sqlite3DbFree(0, pCur->savedKey.mv_data);
    pCur->savedKey.mv_data = NULL;
  }
  if (pCur->stableRow) {
    sqlite3_free(pCur->stableRow);
    pCur->stableRow = NULL;
    pCur->stableRowCap = 0;
  }
  if (pCur->ephCur) ephCursorClose(pCur);
  p = pCur->pBtree;
  if (p && !pCur->prev && !pCur->next && p->first_cursor != pCur) {
    /* sqlite3BtreeCursor failed before linking this cursor into the list, so
    ** there is nothing to unlink, and the list belongs to other cursors. */
    pCur->pBtree = NULL;
    return SQLITE_OK;
  }
  if (p) {
#ifdef LUMO_LMDB_DEBUG
    /* check consistency */
    BtCursor *pList = p->first_cursor, *pPrev = NULL;
    int found = 0;
LUMO_LOG("pCur=%p p=%p first_cursor=%p last_cursor=%p\n", pCur, p, pList, p->last_cursor);
    while (pList) {
      if (pList == pCur) found = 1;
LUMO_LOG("    %p  ->next=%p  ->prev=%p  ->pBtree=%p\n",
	 pList, pList->next, pList->prev, pList->pBtree);
      if (pList->prev != pPrev) {
	LUMO_LOG("sqlite3BtreeCloseCursor(pCur=%p): pList=%p, ->prev=%p != %p\n",
		 pCur, pList, pList->prev, pPrev);
	return SQLITE_INTERNAL;
      }
      if (pList->pBtree != p) {
	LUMO_LOG("sqlite3BtreeCloseCursor(pCur=%p): pList=%p, ->pBtree=%p != %p\n",
		 pCur, pList, pList->pBtree, p);
	return SQLITE_INTERNAL;
      }
      pPrev = pList;
      pList = pList->next;
    }
    if (p->last_cursor != pPrev) {
      LUMO_LOG("sqlite3BtreeCloseCursor(pCur=%p): last=%p != %p\n",
	       pCur, pPrev, p->last_cursor);
      return SQLITE_INTERNAL;
    }
    if (! found) {
      LUMO_LOG("sqlite3BtreeCloseCursor(pCur=%p): cursor not in tree's list (p=%p)\n",
	       pCur, p);
      return SQLITE_INTERNAL;
    }
#endif
    if (pCur->prev)
      pCur->prev->next = pCur->next;
    else
      p->first_cursor = pCur->next;
    if (pCur->next)
      pCur->next->prev = pCur->prev;
    else
      p->last_cursor = pCur->prev;
    pCur->pBtree = NULL;
#ifdef BTREE_SINGLE
    /* SQLite creates BTREE_SINGLE ephemeral btrees for temp workspaces
     * (window functions, GROUP BY spill, DISTINCT, etc.) and opens
     * multiple cursors on them via OP_OpenDup.  Only close the btree
     * when the last cursor on it is closed, so that the next cursor
     * close path on the same btree does not double-abort its txn and
     * double-free the Btree pointer itself. */
    if ((p->flags & BTREE_SINGLE) && p->first_cursor == NULL) {
      LUMO_LOG("sqlite3BtreeCloseCursor: BTREE_SINGLE + last cursor: closing btree\n");
      sqlite3BtreeClose(p);
    }
#endif
  } else {
    LUMO_LOG("sqlite3BtreeCloseCursor: pCur->pBtree is NULL! (pCur=%p)\n", pCur);
  }
  return SQLITE_OK;
}

/* restore cursor if needed; positioning may be skipped if the next operation
 * would be a move */
int restoreCursor(BtCursor *pCur, int reposition) {
  Btree *p = pCur->pBtree;
  int rc;
  /* if we weren't supposed to use this cursor, well, don't */
  if (pCur->tripCode != SQLITE_OK) return pCur->tripCode;
  /* if the cursor is valid, that'll be all */
  if (!pCur->savedKey.mv_data) return SQLITE_OK;
  /* The cursor is being rebuilt in a new transaction; any cached current row
  ** points into the old txn's mapping and must not be reused. */
  cursorCacheClear(pCur);
  /* pCur->cursor should be NULL; if it isn't, something went wrong
  ** somewhere and we fix it now */
  if (pCur->cursor) mdb_cursor_close(pCur->cursor);
  pCur->cursor = NULL;
  rc = get_table(p, pCur->rootPage, pCur->pKeyInfo ? get_table_index : 0, &pCur->dbi);
  if (rc) return rc;
  rc = mdb_cursor_open(p->svp->txn, pCur->dbi, &pCur->cursor);
  if (rc) return error_map(rc);
  if (reposition) {
    MDB_val key[2], data;
    UnpackedRecord *pUnpacked = NULL;
    key[0] = pCur->savedKey;
    key[1].mv_data = NULL;
    if (pCur->pKeyInfo) {
      /* index_compare, used by index and WITHOUT ROWID tables, reads key[1] as
      ** an UnpackedRecord*, and the MDB_SET reseek below calls it, so key[1]
      ** must hold the unpacked form of the saved key. */
      pUnpacked = sqlite3VdbeAllocUnpackedRecord(pCur->pKeyInfo);
      if (! pUnpacked) return SQLITE_NOMEM_BKPT;
      /* 3.51.0 dropped the KeyInfo* first argument; the KeyInfo travels
       * with the UnpackedRecord allocated above. */
#if SQLITE_VERSION_NUMBER < 3051000
      sqlite3VdbeRecordUnpack(pCur->pKeyInfo, (int)pCur->savedKey.mv_size,
                              pCur->savedKey.mv_data, pUnpacked);
#else
      sqlite3VdbeRecordUnpack((int)pCur->savedKey.mv_size,
                              pCur->savedKey.mv_data, pUnpacked);
#endif
      key[1].mv_data = (void *)pUnpacked;
    }
    rc = mdb_cursor_get(pCur->cursor, key, &data, MDB_SET);
    if (pUnpacked) sqlite3DbFree(pCur->pKeyInfo->db, pUnpacked);
    if (rc) return error_map(rc);
  }
  sqlite3DbFree(0, pCur->savedKey.mv_data);
  pCur->savedKey.mv_data = NULL;
  return SQLITE_OK;
}

/*
** Move the cursor to the entry with rowid intKey, or to the next larger
** entry, and set *pRes to 0 for an exact match or 1 for a larger entry.  If
** every rowid is smaller, move to the last entry and set *pRes to -1; an
** empty table leaves the cursor at EOF.  biasRight is ignored.  An ephemeral
** btree may leave the cursor on a smaller entry, with *pRes negative.
*/
int sqlite3BtreeTableMoveto(
  BtCursor *pCur,          /* The cursor to be moved */
  i64 intKey,              /* The table key */
  int biasRight,           /* If true, bias the search to the high end */
  int *pRes                /* Write search results here */
){
  unsigned char b64[8];
  MDB_val key[2], data;
  u64 keyVal = intKey;
  int rc;
  if (pCur->pBtree->pEph) {
    rc = ephSeekInt((EphCursor *)pCur->ephCur, intKey, pRes);
    pCur->atEof = (((EphCursor *)pCur->ephCur)->cur == NULL);
    return rc;
  }
  cursorCacheClear(pCur);
  pCur->cleared = 0;
  if (pCur->incrblobTripped) {
    rc = incrblobReseek(pCur);
    if (rc != SQLITE_OK) return rc;
  }
  rc = restoreCursor(pCur, 0);
  if (rc != SQLITE_OK) return rc;
  pCur->atEof = 0;
  put8(key, b64, keyVal);
  data.mv_size = 0;
  data.mv_data = "";
  rc = mdb_cursor_get(pCur->cursor, key, &data, MDB_SET_RANGE);
  if (rc == 0) {
    /* see if the match is exact */
    *pRes = keyVal != get8(key);
    cursorCacheSet(pCur, &key[0], &data);
    LUMO_LOG("sqlite3BtreeTableMoveto idx=%lld bias=%d => found %lld (%d)\n",
	     (long long)keyVal, biasRight, (long long)get8(key), *pRes);
    return SQLITE_OK;
  }
  if (rc != MDB_NOTFOUND) {
    LUMO_LOG("sqlite3BtreeTableMoveto idx=%d bias=%d => failed (MDB_SET_RANGE) %d\n",
	     (int)intKey, biasRight, rc);
    return error_map(rc);
  }
  /* MDB_SET_RANGE returned MDB_NOTFOUND: every rowid is smaller than intKey.
  ** Move to the last row and report *pRes < 0, which OP_SeekLE and OP_SeekLT
  ** accept as the answer.  On an empty table MDB_LAST also returns
  ** MDB_NOTFOUND, so atEof is set and sqlite3BtreeEof() reports it. */
  rc = mdb_cursor_get(pCur->cursor, key, &data, MDB_LAST);
  if (rc == MDB_NOTFOUND) {
    pCur->atEof = 1;
  } else if (rc != 0) {
    LUMO_LOG("sqlite3BtreeTableMoveto idx=%d bias=%d => MDB_LAST failed %d\n",
	     (int)intKey, biasRight, rc);
    return error_map(rc);
  } else {
    cursorCacheSet(pCur, &key[0], &data);
  }
  *pRes = -1;
  LUMO_LOG("sqlite3BtreeTableMoveto idx=%d bias=%d => not found, %s\n",
	   (int)intKey, biasRight,
	   pCur->atEof ? "table empty" : "cursor at last row");
  return SQLITE_OK;
}

/*
** Move the cursor to the entry matching pIdxKey, or to the nearest entry.
** MDB_SET_RANGE positions the cursor on the first entry that compares
** >= pIdxKey under index_compare, and *pRes is that entry's comparison
** with pIdxKey, so 0 or positive: when every compared field is equal it is
** default_rc, except that a full-key search of a spilled WITHOUT ROWID
** table reports 0.  If every entry is smaller the cursor is left on the last
** entry with *pRes negative; on an empty index *pRes is negative and
** sqlite3BtreeEof() reports true.  pIdxKey->eqSeen is set if an entry equal
** on the compared fields exists.  An ephemeral btree may leave the cursor on
** a smaller entry, with *pRes negative.
*/
int sqlite3BtreeIndexMoveto(
  BtCursor *pCur,          /* The cursor to be moved */
  UnpackedRecord *pIdxKey, /* Unpacked index key */
  int *pRes                /* Write search results here */
){
  MDB_val key[2], data;
  int rc;
  if (pCur->pBtree->pEph) {
    /* ephSeek leaves the cursor on the nearest node with *pRes set to its
    ** signed comparison against pIdxKey, which may be negative, and sets
    ** pIdxKey->eqSeen on an exact match.  OP_Found, OP_IdxDelete and the
    ** OP_Seek opcodes accept this result. */
    int rc2 = ephSeek((EphCursor *)pCur->ephCur, pIdxKey, pRes);
    pCur->atEof = (((EphCursor *)pCur->ephCur)->cur == NULL);
    return rc2;
  }
  cursorCacheClear(pCur);
  pCur->cleared = 0;
  rc = restoreCursor(pCur, 0);
  if (rc != SQLITE_OK) return rc;
  pCur->atEof = 0;
  key[0].mv_size = 1;
  key[0].mv_data = "";
  key[1].mv_data = pIdxKey;
  pIdxKey->errCode = 0;     /* as SQLite's own sqlite3BtreeIndexMoveto() does */
  data.mv_size = 0;
  data.mv_data = "";
  rc = mdb_cursor_get(pCur->cursor, key, &data, MDB_SET_RANGE);
  if (rc == 0) {
    /* key[0] is the first stored entry >= the search key, so *pRes, the
    ** signed comparison of that entry against the search key, is 0 or
    ** positive. */
    int cmp;
    /* Compare the landed stored key against the search key.  The stored key is
    ** the primary-key record for a spilled WITHOUT ROWID table, or the whole
    ** record for a plain index, and it is authoritative for ordering.  For a
    ** spilled table set default_rc to 0 so an exact PK match reports 0 (see the
    ** long comment in index_compare).  This applies only to a full-key search
    ** record.  A prefix seek keeps default_rc so it sorts to the group edge. */
    /* index_compare restores eqSeen after every call, because LMDB also
    ** compares against branch-page separators, so eqSeen is set here from the
    ** landed entry, comparing with default_rc 0.  A seek with default_rc -1
    ** (OP_SeekLE, OP_SeekGT) lands on the entry after the matching group, so
    ** when cmpEq>0 the preceding entry is tested via MDB_PREV, and MDB_NEXT
    ** returns the cursor to the landed entry.  An eqOnly seek
    ** (BTREE_SEEK_EQ) reports no match unless eqSeen is set. */
    {
      i8 saved = pIdxKey->default_rc;
      int cmpEq;
      pIdxKey->default_rc = 0;
      cmpEq = lumoStoredKeyCompare(pCur, (int)key[0].mv_size, key[0].mv_data,
                                   pIdxKey);
      /* A record comparison returns default_rc only when every compared field
      ** is equal, so cmp, described above, differs from cmpEq only when cmpEq
      ** is 0. */
      if (cmpEq != 0 || pIdxKey->errCode) {
        cmp = cmpEq;
      } else if (lumoCursorSpills(pCur)
                 && pIdxKey->nField >= pCur->pKeyInfo->nKeyField) {
        cmp = 0;
      } else {
        cmp = saved;
      }
      *pRes = cmp;
      if (cmpEq == 0) {
        pIdxKey->eqSeen = 1;
      } else if (cmpEq > 0) {
        MDB_val pkey[2], pdata;
        int prc = mdb_cursor_get(pCur->cursor, pkey, &pdata, MDB_PREV);
        if (prc == 0) {
          if (lumoStoredKeyCompare(pCur, (int)pkey[0].mv_size, pkey[0].mv_data,
                                   pIdxKey) == 0) {
            pIdxKey->eqSeen = 1;
          }
          /* restore the cursor to the SET_RANGE landing position */
          mdb_cursor_get(pCur->cursor, key, &data, MDB_NEXT);
        }
      }
      pIdxKey->default_rc = saved;
    }
    cursorCacheSet(pCur, &key[0], &data);
#ifdef LUMO_LMDB_DEBUG
    {
      i64 seekv = (pIdxKey->nField>0 && (pIdxKey->aMem[0].flags & MEM_Int))
                   ? pIdxKey->aMem[0].u.i : -99999;
      LUMO_LOG("sqlite3BtreeIndexMoveto SETRANGE: seekv=%lld nField=%d default_rc=%d cmp=%d landed(%zd)",
               (long long)seekv, (int)pIdxKey->nField, (int)pIdxKey->default_rc, cmp, (size_t)key[0].mv_size);
      LUMO_LOG_DATA(key[0].mv_size, key[0].mv_data);
      LUMO_LOG("\n");
    }
#endif
    LUMO_LOG("sqlite3BtreeIndexMoveto => found (%d)\n", *pRes);
    return SQLITE_OK;
  }
  if (rc != MDB_NOTFOUND) {
    LUMO_LOG("sqlite3BtreeIndexMoveto => failed (MDB_SET_RANGE) %d\n", rc);
    return error_map(rc);
  }
  /* Every entry is smaller than the search key.  Leave the cursor on the
  ** last entry with *pRes negative, as sqlite3BtreeTableMoveto does. */
  rc = mdb_cursor_get(pCur->cursor, key, &data, MDB_LAST);
  if (rc == MDB_NOTFOUND) {
    pCur->atEof = 1;
  } else if (rc != 0) {
    LUMO_LOG("sqlite3BtreeIndexMoveto => MDB_LAST failed %d\n", rc);
    return error_map(rc);
  } else {
    cursorCacheSet(pCur, &key[0], &data);
    /* The seek key sorts past every stored key, so any matching rows are at
    ** the tail.  The last row is the largest.  If it matches the seek prefix
    ** (compare ignoring default_rc) then an equal entry exists, record it so
    ** eqOnly/BTREE_SEEK_EQ seeks do not wrongly report "not found".  See the
    ** companion comment in the MDB_SET_RANGE branch above. */
    {
      i8 saved = pIdxKey->default_rc;
      pIdxKey->default_rc = 0;
      if (lumoStoredKeyCompare(pCur, (int)key[0].mv_size, key[0].mv_data,
                               pIdxKey) == 0) {
        pIdxKey->eqSeen = 1;
      }
      pIdxKey->default_rc = saved;
    }
  }
  *pRes = -1;
  LUMO_LOG("sqlite3BtreeIndexMoveto => not found, %s\n",
           pCur->atEof ? "table empty" : "cursor at last row");
  return SQLITE_OK;
}

/* Entry point used by SQLite before 3.37, which replaced it with
** sqlite3BtreeTableMoveto and sqlite3BtreeIndexMoveto. */
int sqlite3BtreeMovetoUnpacked(
  BtCursor *pCur,          /* The cursor to be moved */
  UnpackedRecord *pIdxKey, /* Unpacked index key */
  i64 intKey,              /* The table key */
  int biasRight,           /* If true, bias the search to the high end */
  int *pRes                /* Write search results here */
){
  if (pIdxKey)
    return sqlite3BtreeIndexMoveto(pCur, pIdxKey, pRes);
  else
    return sqlite3BtreeTableMoveto(pCur, intKey, biasRight, pRes);
}

int sqlite3BtreeCursorHasMoved(BtCursor *pCur) {
  MDB_val key, data;
  int rc;
  if (pCur == &fakeCursor) {
    LUMO_LOG("sqlite3BtreeCursorHasMoved (fake): 0\n");
    return 0;
  }
  if (pCur->pBtree && pCur->pBtree->pEph) {
    /* Ephemeral cursors have no LMDB cursor and are not invalidated when a
    ** transaction ends, so they never move.  sqlite3BtreeCursorRestore reports
    ** the same row for an ephemeral cursor, so reporting movement here would
    ** make OP_Column restart and call this function again indefinitely. */
    return 0;
  }
  /* curValid means the row was cached by the last positioning operation
  ** and not since cleared by restoreCursor, invalidateCursors or a write
  ** through this cursor.  A write through a sibling cursor does not clear
  ** it; see cursorCacheClearAfterWrite.  curValid==0 does not imply
  ** movement, so fall through to the probe. */
  if (pCur->curValid) {
    LUMO_ASSERT_CACHE_FRESH(pCur);
    LUMO_LOG("sqlite3BtreeCursorHasMoved %u: 0 (cached)\n", pCur->rootPage);
    return 0;
  }
  rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
  LUMO_LOG("sqlite3BtreeCursorHasMoved %u: %d\n", pCur->rootPage, rc);
  if (rc) return 1;
  return 0;
}

int sqlite3BtreeCursorRestore(BtCursor *pCur, int *pDifferentRow) {
  int rc;
  LUMO_LOG("called: sqlite3BtreeCursorRestore\n");
  if (pCur->pBtree && pCur->pBtree->pEph) {
    /* Ephemeral cursors never move out from under themselves (see
    ** sqlite3BtreeCursorHasMoved), so there is nothing to restore and the row
    ** is unchanged.  Belt-and-braces in case a caller reaches here directly. */
    *pDifferentRow = 0;
    return SQLITE_OK;
  }
  /* restoreCursor(pCur, 1) reseeks with MDB_SET.  A saved row deleted since
  ** returns SQLITE_NOTFOUND, and restoreCursor keeps savedKey on that path,
  ** so it is freed here and the cursor is left at EOF with *pDifferentRow
  ** set; upstream instead leaves the cursor on a nearby row.  Any other
  ** error is returned with *pDifferentRow set. */
  *pDifferentRow = 0;
  rc = restoreCursor(pCur, 1);
  if (rc == SQLITE_OK) {
    return SQLITE_OK;                 /* repositioned onto the same row */
  }
  *pDifferentRow = 1;
  if (rc == SQLITE_NOTFOUND) {
    /* saved row was deleted: not an error, just a moved cursor */
    if (pCur->savedKey.mv_data) {
      sqlite3DbFree(0, pCur->savedKey.mv_data);
      pCur->savedKey.mv_data = NULL;
    }
    pCur->atEof = 1;                  /* cursor no longer on a valid row */
    return SQLITE_OK;
  }
  return rc;                          /* genuine error (I/O, abort, ...) */
}

/*
** Delete the entry the cursor points to.  After mdb_cursor_del, MDB_NEXT
** returns the entry after the deleted one and MDB_PREV the entry before
** it, which is the BTREE_SAVEPOSITION behaviour, so flags are ignored.
*/
int sqlite3BtreeDelete(BtCursor *pCur, u8 flags) {
  int rc;
  if (pCur->pBtree->pEph) return ephDelete((EphCursor *)pCur->ephCur);
  LUMO_LOG("sqlite3BtreeDelete\n");
  cursorCacheClear(pCur);
  rc = restoreCursor(pCur, 1);
  if (rc != SQLITE_OK) {
    LUMO_LOG("sqlite3BtreeDelete: restore: %d\n", rc);
    return rc;
  }
  if (!pCur->pKeyInfo && pCur->pBtree->hasIncrblobCur) {
    invalidateIncrblobCursors(pCur->pBtree, pCur->rootPage,
                              sqlite3BtreeIntegerKey(pCur), 0);
  }
  rc = mdb_cursor_del(pCur->cursor, 0);
  if (rc) {
    LUMO_LOG("sqlite3BtreeDelete: delete: %d\n", rc);
    return error_map(rc);
  }
  cursorCacheClearAfterWrite(pCur);
  LUMO_LOG("sqlite3BtreeDelete: OK\n");
  return SQLITE_OK;
}

/*
** Prepare to copy the current row of pSrc into pDest.  The copy is made
** by the next call, which must be sqlite3BtreeInsert() on pDest with
** BTREE_PREFORMAT: it reads the key and value from pSrc verbatim, using
** iKey as the rowid when pDest is a table.
*/
int sqlite3BtreeTransferRow(BtCursor *pDest, BtCursor *pSrc, i64 iKey){
  LUMO_LOG("sqlite3BtreeTransferRow pDest=%p pSrc=%p iKey=%lld\n",
	   pDest, pSrc, (long long)iKey);
  pDest->srcCursor = pSrc;
  pDest->srcKey = iKey;
  return SQLITE_OK;
}

/*
** Insert a row into the table or index pCur is open on, replacing any row
** with an equal key.  For a table, pX->nKey is the rowid and pX->pData,
** nData, nZero the row.  For an index or WITHOUT ROWID table pX->pKey,
** nKey is the record, and pX->pData, nData is stored as Lumo extension
** data when BTREE_LUMO_EXTENSIONS is set; if pX->nMem is non-zero,
** pX->aMem is the unpacked record used for comparison.  mdb_cursor_put
** always finds the insertion point, so seekResult is ignored.
** BTREE_PREFORMAT copies the row saved by sqlite3BtreeTransferRow().  On
** success the LMDB cursor is on the inserted row.
*/
int sqlite3BtreeInsert(
  BtCursor *pCur,                /* Insert data into the table of this cursor */
  const BtreePayload *pX,        /* Content of the row to be inserted */
  int flags,                     /* BTREE_APPEND, BTREE_PREFORMAT, BTREE_SAVEPOSITION */
  int seekResult                 /* Ignored */
){
  MDB_val key[2], data;
  UnpackedRecord *pUnpacked = NULL, aUnpacked;
  unsigned char bKey[8];
  u64 nKey = pX->nKey;
  int rc;
  unsigned int mdbFlags = 0;
  const int lumoData = pCur->pBtree->flags & BTREE_LUMO_EXTENSIONS;
  unsigned char *spillKey = NULL;  /* truncated PK key for WITHOUT ROWID spill */
  int spill = 0;                   /* nonzero => value carries full record */
  u32 spillRecLen = 0;             /* full-record length stored in value */
  cursorCacheClear(pCur);
  if (pCur->pBtree->pEph) {
    /* Ephemeral (BTREE_SINGLE): record is pX->pKey; build/borrow its unpacked
    ** form for the comparator, then insert into the in-RAM tree. */
    UnpackedRecord *pU, aU;
    if (!pCur->pKeyInfo) {
      /* intkey ephemeral table: the key is the rowid in pX->nKey and the row
      ** is pX->pData, nData followed by pX->nZero zero bytes. */
      const unsigned char *pRow = (const unsigned char *)pX->pData;
      unsigned char *pZeroed = NULL;
      u32 nRow = (u32)pX->nData;
      if (pX->nZero > 0) {
        nRow += (u32)pX->nZero;
        pZeroed = sqlite3Malloc(nRow);
        if (!pZeroed) return SQLITE_NOMEM_BKPT;
        if (pX->nData) memcpy(pZeroed, pX->pData, pX->nData);
        memset(pZeroed + pX->nData, 0, pX->nZero);
        pRow = pZeroed;
      }
      rc = ephInsertIntKey(pCur, (i64)pX->nKey, pRow, nRow);
      sqlite3_free(pZeroed);
      return rc;
    }
    if (pX->nMem) {
      aU.pKeyInfo = pCur->pKeyInfo; aU.aMem = pX->aMem; aU.nField = pX->nMem;
      aU.default_rc = 0; aU.errCode = 0; aU.r1 = 0; aU.r2 = 0; aU.eqSeen = 0;
      return ephInsert(pCur, (const unsigned char *)pX->pKey, (u32)pX->nKey, &aU);
    }
    pU = sqlite3VdbeAllocUnpackedRecord(pCur->pKeyInfo);
    if (!pU) return SQLITE_NOMEM_BKPT;
#if SQLITE_VERSION_NUMBER < 3051000
    sqlite3VdbeRecordUnpack(pCur->pKeyInfo, (int)pX->nKey, pX->pKey, pU);
#else
    sqlite3VdbeRecordUnpack((int)pX->nKey, pX->pKey, pU);
#endif
    rc = ephInsert(pCur, (const unsigned char *)pX->pKey, (u32)pX->nKey, pU);
    sqlite3DbFree(pCur->pKeyInfo->db, pU);
    return rc;
  }
#ifdef LUMO_LMDB_LOG_INSERTS
  LUMO_LOG("sqlite3BtreeInsert: data before\n");
  LUMO_LOG_CURSOR(pCur->cursor, pCur->pKeyInfo != NULL);
#endif
  rc = restoreCursor(pCur, 0);
  LUMO_LOG("sqlite3BtreeInsert: restoreCursor: %d\n", rc);
  if (rc != SQLITE_OK) return rc;
  if (!pCur->pKeyInfo && pCur->pBtree->hasIncrblobCur) {
    invalidateIncrblobCursors(pCur->pBtree, pCur->rootPage, pX->nKey, 0);
  }
  if (flags & BTREE_PREFORMAT) {
    /* we are copying a row from pCur->srcCursor so just get key and value
    ** from there */
    rc = restoreCursor(pCur->srcCursor, 1);
    if (rc != SQLITE_OK) {
      LUMO_LOG("restoreCursor: %d\n", rc);
      return rc;
    }
    rc = mdb_cursor_get(pCur->srcCursor->cursor, key, &data, MDB_GET_CURRENT);
    pCur->srcCursor = NULL;
    if (rc) {
      LUMO_LOG("mdb_cursor_get: %d\n", rc);
      return error_map(rc);
    }
    LUMO_LOG("key(%zd):", key[0].mv_size);
    LUMO_LOG_DATA(key[0].mv_size, key[0].mv_data);
    LUMO_LOG("\n");
    LUMO_LOG("data(%zd):", data.mv_size);
    LUMO_LOG_DATA(data.mv_size, data.mv_data);
    LUMO_LOG("\n");
    nKey = pCur->srcKey;
  }
  if (pCur->pKeyInfo) {
    /* inserting into an index or a WITHOUT ROWID table
    ** we allow the caller to provide data as well as key and this
    ** will be considered a Lumo column.  The caller can retrieve
    ** that but only if they know to ask for it */
    /* For BTREE_PREFORMAT, key[0] and data were read from pCur->srcCursor
    ** above, and pX->pKey does not hold the row.  index_compare reads key[1]
    ** as an UnpackedRecord, so unpack the source key from key[0] for it. */
    if (flags & BTREE_PREFORMAT) {
      pUnpacked = sqlite3VdbeAllocUnpackedRecord(pCur->pKeyInfo);
      if (! pUnpacked) return SQLITE_NOMEM_BKPT;
      /* 3.51.0 dropped the KeyInfo* first argument; see note at the
       * earlier call site. */
#if SQLITE_VERSION_NUMBER < 3051000
      sqlite3VdbeRecordUnpack(pCur->pKeyInfo, (int)key[0].mv_size, key[0].mv_data, pUnpacked);
#else
      sqlite3VdbeRecordUnpack((int)key[0].mv_size, key[0].mv_data, pUnpacked);
#endif
      key[1].mv_data = (void *)pUnpacked;
      goto insert_it;
    }
    if (pX->nMem) {
      aUnpacked.pKeyInfo = pCur->pKeyInfo;
      aUnpacked.aMem = pX->aMem;
      aUnpacked.nField = pX->nMem;
      aUnpacked.default_rc = 0;
      aUnpacked.errCode = 0;
      aUnpacked.r1 = 0;
      aUnpacked.r2 = 0;
      aUnpacked.eqSeen = 0;
    } else {
      /* no unpacked key supplied; build one here for the comparator */
      pUnpacked = sqlite3VdbeAllocUnpackedRecord(pCur->pKeyInfo);
      if (! pUnpacked) return SQLITE_NOMEM_BKPT;
      /* 3.51.0 dropped the KeyInfo* first argument; pUnpacked from the
       * preceding AllocUnpackedRecord carries it. */
#if SQLITE_VERSION_NUMBER < 3051000
      sqlite3VdbeRecordUnpack(pCur->pKeyInfo, (int)pX->nKey, pX->pKey, pUnpacked);
#else
      sqlite3VdbeRecordUnpack((int)pX->nKey, pX->pKey, pUnpacked);
#endif
      if (pUnpacked->nField==0 || pUnpacked->nField>pCur->pKeyInfo->nAllField) {
	rc = SQLITE_CORRUPT_BKPT;
	goto out;
      }
    }
    LUMO_LOG("sqlite3BtreeInsert(%u) key(%zd)", pCur->rootPage, (size_t)pX->nKey);
    LUMO_LOG_DATA(pX->nKey, pX->pKey);
    if (lumoData) {
      LUMO_LOG(" -- data(%zd)", (size_t)pX->nData);
      LUMO_LOG_DATA(pX->nData, pX->pData);
    }
    LUMO_LOG("\n");

    /* Choose the storage split.  When lumoCursorSpills() is true
    ** (nKeyField < nAllField: a WITHOUT ROWID table, or a UNIQUE index whose
    ** columns are all NOT NULL) the LMDB key is the first nKeyField columns
    ** and the value carries the full record; see "Spill-to-value record
    ** format" above.  Otherwise the whole record is the key. */
    spill = lumoCursorSpills(pCur);
    if (spill) {
      int nTrunc = 0;
      rc = lumoTruncateRecord((const unsigned char *)pX->pKey, (int)pX->nKey,
                              pCur->pKeyInfo->nKeyField, &spillKey, &nTrunc);
      if (rc != SQLITE_OK) goto out;
      key[0].mv_size = nTrunc;
      key[0].mv_data = spillKey;
      spillRecLen = (u32)pX->nKey;            /* full record goes in value */
    } else {
      key[0].mv_size = pX->nKey;
      key[0].mv_data = (void *)pX->pKey;
    }
    /* LMDB caps the stored key length: 511 bytes in LMDB 0.9, derived from
    ** the page size in LMDB 1.0.  A longer key, from a wide index or primary
    ** key, makes the insert return SQLITE_TOOBIG. */
    if ((i64)key[0].mv_size > (i64)mdb_env_get_maxkeysize(pCur->pBtree->env)) {
      rc = SQLITE_TOOBIG;
      goto out;
    }
    key[1].mv_data = (void *)(pUnpacked ? pUnpacked : &aUnpacked);
    /* The stored key holds only nKeyField columns, so compare on that many. */
    if (spill) {
      UnpackedRecord *pCmp = pUnpacked ? pUnpacked : &aUnpacked;
      if (pCmp->nField > pCur->pKeyInfo->nKeyField)
        pCmp->nField = pCur->pKeyInfo->nKeyField;
    }
    /* Value = [varint(recLen) || record  (spill only)] || varint(0) || trailer */
    data.mv_size = 0;
    if (spill) data.mv_size += sqlite3VarintLen(spillRecLen) + spillRecLen;
    data.mv_size += sqlite3VarintLen(0);
    if (lumoData) data.mv_size += pX->nData;
    mdbFlags = MDB_RESERVE;

    /* Delete any entry with an equal primary key before the put.  When the
    ** new value is the same size as the old value, mdb_cursor_put overwrites
    ** only the value and keeps the old key bytes; deleting first stores the
    ** new key, as SQLite's whole-cell replacement does. */
    if (spill) {
      MDB_cursor *dcur = NULL;
      if (mdb_cursor_open(pCur->txn, pCur->dbi, &dcur) == 0) {
        MDB_val fkey[2], fdata;
        UnpackedRecord *pSeek = pUnpacked ? pUnpacked : &aUnpacked;
        fkey[0] = key[0];               /* the PK-only key */
        fkey[1].mv_data = (void *)pSeek; /* compare on nKeyField (already capped) */
        fdata.mv_size = 0; fdata.mv_data = "";
        if (mdb_cursor_get(dcur, fkey, &fdata, MDB_SET) == 0) {
          /* exact PK match present: delete it before reinserting */
          mdb_cursor_del(dcur, 0);
        }
        mdb_cursor_close(dcur);
      }
    }
  } else {
    /* inserting into a WITH ROWID table */
    put8(key, bKey, nKey);
    LUMO_LOG("sqlite3BtreeInsert(%u) key=%lld:", pCur->rootPage, (long long)nKey);
    LUMO_LOG_DATA(key[0].mv_size, key[0].mv_data);
    if (flags & BTREE_PREFORMAT) {
      LUMO_LOG(", data(%zd):", data.mv_size);
      LUMO_LOG_DATA(data.mv_size, data.mv_data);
      LUMO_LOG("\n");
    } else {
      LUMO_LOG(", data(%d+%d):", (int)pX->nData, (int)pX->nZero);
      LUMO_LOG_DATA(pX->nData, pX->pData);
      LUMO_LOG("\n");
      data.mv_size = pX->nData + pX->nZero;
      if (pX->nZero) {
	mdbFlags = MDB_RESERVE;
      } else {
	data.mv_data = (void *)pX->pData;
      }
    }
  }
insert_it:
  /* For a table cursor with BTREE_APPEND use MDB_APPEND: LMDB descends the
  ** rightmost path and compares the key only with the last key, without
  ** searching for it.  A key not greater than the last key returns
  ** MDB_KEYEXIST, and the ordinary put is used. */
  if ((flags & BTREE_APPEND) && !pCur->pKeyInfo) {
    rc = mdb_cursor_put(pCur->cursor, key, &data, mdbFlags | MDB_APPEND);
    if (rc == MDB_KEYEXIST) {
      rc = mdb_cursor_put(pCur->cursor, key, &data, mdbFlags);
    }
  } else {
    rc = mdb_cursor_put(pCur->cursor, key, &data, mdbFlags);
  }
  if (rc) {
    LUMO_LOG("sqlite3BtreeInsert(%u): failed(%d)\n", pCur->rootPage, rc);
    goto out_map;
  }
  pCur->cleared = 0;        /* the LMDB cursor is on the inserted row */
  if (! (flags & BTREE_PREFORMAT)) {
    if (pCur->pKeyInfo) {
      /* Value layout:
      **   [ varint(recLen) || fullRecord ]   (spill only)
      **   varint(0) placeholder
      **   lumoTrailer                         (if lumoData)
      ** The full record is needed so the payload accessors can return the
      ** row for a WITHOUT ROWID table whose key holds only the PK columns. */
      unsigned char * ptr = data.mv_data;
      if (spill) {
        ptr += putVarint32(ptr, spillRecLen);
        memcpy(ptr, pX->pKey, spillRecLen);
        ptr += spillRecLen;
      }
      ptr += putVarint32(ptr, 0);
      if (lumoData)
	memcpy(ptr, pX->pData, pX->nData);
      LUMO_LOG("  ==> LMDB key(%zd)", (size_t)key[0].mv_size);
      LUMO_LOG_DATA(key[0].mv_size, key[0].mv_data);
      LUMO_LOG("  -- data(%zd)", (size_t)data.mv_size);
      LUMO_LOG_DATA(data.mv_size, data.mv_data);
      LUMO_LOG("\n");
    } else if (pX->nZero) {
      /* copy data and the required number of zeros */
      unsigned char * ptr = data.mv_data;
      memcpy(ptr, pX->pData, pX->nData);
      ptr += pX->nData;
      memset(ptr, 0, pX->nZero);
    }
  }
  LUMO_LOG("sqlite3BtreeInsert(%u): OK\n", pCur->rootPage);
  cursorCacheClearAfterWrite(pCur);
  rc = SQLITE_OK;
  goto out;
out_map:
  rc = error_map(rc);
out:
  if (spillKey) sqlite3_free(spillKey);
  if (pUnpacked)
    sqlite3DbFree(pCur->pKeyInfo->db, pUnpacked);
#ifdef LUMO_LMDB_LOG_INSERTS
  LUMO_LOG("sqlite3BtreeInsert: data after\n");
  LUMO_LOG_CURSOR(pCur->cursor, pCur->pKeyInfo != NULL);
#endif
  return rc;
}

/* common code for cursor moves to first, last, next, prev */
static int cursorMove(BtCursor *pCur, int *pRes, MDB_cursor_op op, int reposition) {
  MDB_val key, data;
  int rc;
  cursorCacheClear(pCur);
  /* A cleared cursor (after sqlite3BtreeClearCursor) has no current position.
  ** A relative move from it reports EOF.  An absolute move establishes a fresh
  ** position and clears the flag. */
  if (pCur->cleared) {
    if (op == MDB_NEXT || op == MDB_PREV) {
      pCur->atEof = *pRes = 1;
      return SQLITE_OK;
    }
    pCur->cleared = 0;
  }
  /* restoreCursor is needed only when the cursor is tripped or
  ** invalidateCursors saved its position at a transaction or savepoint
  ** change; testing tripCode and savedKey here avoids a call per row on a
  ** scan. */
  if (pCur->tripCode != SQLITE_OK || pCur->savedKey.mv_data) {
    rc = restoreCursor(pCur, reposition);
    if (rc != SQLITE_OK) return rc;
  }
  rc = mdb_cursor_get(pCur->cursor, &key, &data, op);
  if (rc) {
    if (rc != MDB_NOTFOUND) {
      LUMO_LOG("cursorMove %d (%u): fail %d\n", (int)op, pCur->rootPage, rc);
      return error_map(rc);
    }
    pCur->atEof = *pRes = 1;
  } else {
    pCur->atEof = *pRes = 0;
    /* Cache the landed row so PayloadSize/PayloadFetch/IntegerKey need not
    ** re-issue mdb_cursor_get(MDB_GET_CURRENT) for it. */
    cursorCacheSet(pCur, &key, &data);
  }
  LUMO_LOG("cursorMove %d (%u): %d\n", (int)op, pCur->rootPage, *pRes);
  return SQLITE_OK;
}

/*
**  Move the cursor to the first entry in the table.  Return SQLITE_OK
** on success.  Set *pRes to 0 if the cursor actually points to something
** or set *pRes to 1 if the table is empty.
*/
int sqlite3BtreeFirst(BtCursor *pCur, int *pRes) {
  if (pCur->pBtree->pEph) {
    ephFirst((EphCursor *)pCur->ephCur, pRes);
    pCur->atEof = (*pRes != 0);
    return SQLITE_OK;
  }
  return cursorMove(pCur, pRes, MDB_FIRST, 0);
}

/*
**  Move the cursor to the last entry in the table.  Return SQLITE_OK
** on success.  Set *pRes to 0 if the cursor actually points to something
** or set *pRes to 1 if the table is empty.
*/
int sqlite3BtreeLast(BtCursor *pCur, int *pRes) {
  if (pCur->pBtree->pEph) {
    ephLast((EphCursor *)pCur->ephCur, pRes);
    pCur->atEof = (*pRes != 0);
    return SQLITE_OK;
  }
  return cursorMove(pCur, pRes, MDB_LAST, 0);
}

int sqlite3BtreeNext(BtCursor *pCur, int flags) {
  int pRes, rc;
  if (pCur->pBtree->pEph) {
    rc = ephNext((EphCursor *)pCur->ephCur, &pRes);
    if (rc != SQLITE_OK) return rc;
    pCur->atEof = (pRes != 0);
    return pCur->atEof ? SQLITE_DONE : SQLITE_OK;
  }
  rc = cursorMove(pCur, &pRes, MDB_NEXT, 1);
  if (rc != SQLITE_OK) return rc;
  if (pCur->atEof) return SQLITE_DONE;
  return SQLITE_OK;
}

int sqlite3BtreePrevious(BtCursor *pCur, int flags) {
  int pRes, rc;
  if (pCur->pBtree->pEph) {
    rc = ephPrev((EphCursor *)pCur->ephCur, &pRes);
    if (rc != SQLITE_OK) return rc;
    pCur->atEof = (pRes != 0);
    return pCur->atEof ? SQLITE_DONE : SQLITE_OK;
  }
  rc = cursorMove(pCur, &pRes, MDB_PREV, 1);
  if (rc != SQLITE_OK) return rc;
  if (pCur->atEof) return SQLITE_DONE;
  return SQLITE_OK;
}

int sqlite3BtreeEof(BtCursor *pCur) {
  LUMO_LOG("sqlite3BtreeEof %u: %d\n", pCur->rootPage, (int)pCur->atEof);
  return pCur->atEof;
}

/*
** Return the value of the integer key or "rowid" for a table btree.
** This routine is only valid for a cursor that is pointing into a
** ordinary table btree.  If the cursor points to an index btree or
** is invalid, the result of this routine is undefined.
*/
i64 sqlite3BtreeIntegerKey(BtCursor *pCur) {
  MDB_val key, data;
  i64 res;
  int rc;
  if (pCur->pBtree && pCur->pBtree->pEph) {
    return ephIntKey((EphCursor *)pCur->ephCur);
  }
  /* if the cursor has a saved position, don't bother restoring it, as the
  ** saved position is the required key.  If the payload is actually needed,
  ** we restore the cursor at that point */
  if (pCur->savedKey.mv_data) {
    rc = 0;
    res = get8(&pCur->savedKey);
  } else if (pCur->curValid) {
    LUMO_ASSERT_CACHE_FRESH(pCur);
    /* reuse the row cached by the last positioning op */
    res = get8(&pCur->curKey);
    rc = 0;
  } else {
    rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
    res = rc ? 0 : get8(&key);
  }
  LUMO_LOG("sqlite3BtreeIntegerKey %u: %d (%lld)\n", pCur->rootPage, rc, (long long)res);
  return res;
}

void sqlite3BtreeCursorPin(BtCursor *pCur) {
  LUMO_LOG("called: sqlite3BtreeCursorPin\n");
  /* Cursor pinning is a no-op for the LMDB backend. */
}

void sqlite3BtreeCursorUnpin(BtCursor *pCur) {
  LUMO_LOG("called: sqlite3BtreeCursorUnpin\n");
  /* Cursor pinning is a no-op for the LMDB backend. */
}

/* Copy amt bytes from offset in the current row into pBuf.  SQLite,
** LumoSQL's rowsum check (lumo-vdbeAdd.c) and LumoSQL's OP_Column changes
** (vdbe-changes.mod) call it when sqlite3BtreePayloadFetch returned fewer
** bytes than they need, which this backend does only on error.  In SQLite
** 3.53 and later, sqlite3VdbeFindIndexKey also calls it for each index
** record it compares.  The row comes from the ephemeral tree, the cursor's
** row cache, the saved key of a non-spilled index cursor, or
** MDB_GET_CURRENT. */
int sqlite3BtreePayload(BtCursor *pCur, u32 offset, u32 amt, void *pBuf) {
  MDB_val key, data;
  const unsigned char *src;
  u32 srcSize;
  int rc;
  LUMO_LOG("called: sqlite3BtreePayload offset=%u amt=%u\n", offset, amt);
  if (pCur->pBtree->pEph) {
    src = ephPayload(pCur, &srcSize);
    if (!src || (u64)offset + (u64)amt > (u64)srcSize) return SQLITE_CORRUPT_BKPT;
    memcpy(pBuf, src + offset, amt);
    return SQLITE_OK;
  }
  if (pCur->curValid) {
    LUMO_ASSERT_CACHE_FRESH(pCur);
    /* fast path: the last positioning op cached this row */
    if (pCur->pKeyInfo) {
      lumoIndexRecord(lumoCursorSpills(pCur), &pCur->curKey, &pCur->curData, &src, &srcSize);
    } else {
      src = (const unsigned char *)pCur->curData.mv_data;
      srcSize = (u32)pCur->curData.mv_size;
    }
  } else if (pCur->pKeyInfo) {
    /* Index cursor.  For a plain index the payload is the key, and the
    ** saved-key shortcut avoids stepping the cursor.  For a spilled WITHOUT
    ** ROWID table the full record lives in the value, so we must have a live
    ** cursor positioned on the row (the saved key holds only the PK columns
    ** and cannot serve as the payload). */
    if (pCur->savedKey.mv_data && !lumoCursorSpills(pCur)) {
      key = pCur->savedKey;
      data.mv_data = NULL; data.mv_size = 0;
    } else {
      rc = restoreCursor(pCur, 1);
      if (rc != SQLITE_OK) return rc;
      rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
      if (rc) return SQLITE_IOERR_READ;
    }
    lumoIndexRecord(lumoCursorSpills(pCur), &key, &data, &src, &srcSize);
  } else {
    /* table cursor: payload is the data. */
    rc = restoreCursor(pCur, 1);
    if (rc != SQLITE_OK) return rc;
    rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
    if (rc) return SQLITE_IOERR_READ;
    src = (const unsigned char *)data.mv_data;
    srcSize = (u32)data.mv_size;
  }
  /* SQLite's callers keep the window within the payload, which upstream
  ** asserts; a window outside the payload is reported as corruption. */
  if ((u64)offset + (u64)amt > (u64)srcSize) {
    LUMO_LOG("sqlite3BtreePayload: out of range offset=%u amt=%u size=%u\n",
             offset, amt, srcSize);
    return SQLITE_CORRUPT_BKPT;
  }
  memcpy(pBuf, src + offset, amt);
  return SQLITE_OK;
}

/*
** Return a pointer to the current row's payload, with its length in *pAmt:
** the data for a table cursor, the record for an index cursor.  The whole
** payload is returned, because LMDB returns a value stored on overflow
** pages as one contiguous region.  The pointer is into LMDB's memory,
** which LMDB keeps valid until the next update or the end of the
** transaction, or into the cursor's saved key.  For a BTREE_SINGLE btree
** it is a copy owned by the cursor, or the raw pointer if the copy cannot
** be allocated.  On error *pAmt is 0.
*/
const void *sqlite3BtreePayloadFetch(BtCursor *pCur, u32 *pAmt){
  MDB_val key, data;
  int rc;
  const unsigned char *rec;
  u32 nRec;
  if (pCur->pBtree->pEph) {
    rec = ephPayload(pCur, pAmt);
    return rec ? (const void *)cursorStableRow(pCur, rec, *pAmt) : "";
  }
  if (pCur->curValid) {
    LUMO_ASSERT_CACHE_FRESH(pCur);
    /* fast path: the last positioning op cached this row */
    if (pCur->pKeyInfo) {
      lumoIndexRecord(lumoCursorSpills(pCur), &pCur->curKey, &pCur->curData, &rec, &nRec);
      *pAmt = nRec;
      return cursorMaybeStableRow(pCur, rec, nRec);
    }
    *pAmt = pCur->curData.mv_size;
    return cursorMaybeStableRow(pCur, pCur->curData.mv_data, pCur->curData.mv_size);
  }
  if (pCur->pKeyInfo) {
    /* For a plain index, the saved key is the payload (avoid stepping the
    ** cursor).  For a spilled WITHOUT ROWID table the payload is in the
    ** value, so we need a live, positioned cursor. */
    if (pCur->savedKey.mv_data && !lumoCursorSpills(pCur)) {
      key = pCur->savedKey;
      data.mv_data = NULL; data.mv_size = 0;
      LUMO_LOG("sqlite3BtreePayloadFetch %u: using saved key\n", pCur->rootPage);
      goto skip_fetch;
    }
  } else {
    /* check if we need to restore the cursor */
    rc = restoreCursor(pCur, 1);
    LUMO_LOG("sqlite3BtreePayloadFetch %u: restore: %d\n", pCur->rootPage, rc);
    if (rc != SQLITE_OK) {
      *pAmt = 0;
      return "";
    }
  }
  if (pCur->pKeyInfo) {
    /* index cursor without a usable saved key: restore it before reading */
    rc = restoreCursor(pCur, 1);
    if (rc != SQLITE_OK) { *pAmt = 0; return ""; }
  }
  rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
  LUMO_LOG("sqlite3BtreePayloadFetch %u: %d\n", pCur->rootPage, rc);
  if (rc) {
    *pAmt = 0;
    return "";
  }
skip_fetch:
#ifdef LUMO_LMDB_DEBUG
  if (! pCur->pKeyInfo) {
    LUMO_LOG("ID(%lld) ", (long long)get8(&key));
    LUMO_LOG_DATA(key.mv_size, key.mv_data);
  }
#endif
  if (pCur->pKeyInfo) {
    lumoIndexRecord(lumoCursorSpills(pCur), &key, &data, &rec, &nRec);
    *pAmt = nRec;
    return cursorMaybeStableRow(pCur, rec, nRec);
  } else {
    *pAmt = data.mv_size;
    return cursorMaybeStableRow(pCur, data.mv_data, data.mv_size);
  }
}

/*
** Return the payload size of the current row: the data size for a table
** cursor, or the record size for an index cursor, which for a spilled
** index is the full record held in the value.
*/
u32 sqlite3BtreePayloadSize(BtCursor *pCur){
  MDB_val key, data;
  int rc;
  const unsigned char *rec;
  u32 nRec;
  if (pCur->pBtree->pEph) {
    rec = ephPayload(pCur, &nRec);
    return nRec;
  }
  if (pCur->curValid) {
    LUMO_ASSERT_CACHE_FRESH(pCur);
    /* fast path: the last positioning op cached this row */
    if (pCur->pKeyInfo) {
      lumoIndexRecord(lumoCursorSpills(pCur), &pCur->curKey, &pCur->curData, &rec, &nRec);
      return nRec;
    }
    return pCur->curData.mv_size;
  }
  if (pCur->pKeyInfo) {
    /* Plain index: the saved key's size is the payload size.  Spilled
    ** WITHOUT ROWID: the saved key is only the PK, so we must read the
    ** value to learn the full record length. */
    if (pCur->savedKey.mv_data && !lumoCursorSpills(pCur))
      return pCur->savedKey.mv_size;
    rc = restoreCursor(pCur, 1);
    if (rc != SQLITE_OK) return 0;
  } else {
    /* check if we need to restore the cursor */
    rc = restoreCursor(pCur, 1);
    LUMO_LOG("sqlite3BtreePayloadSize %u: restore: %d\n", pCur->rootPage, rc);
    if (rc != SQLITE_OK) return 0;
  }
  rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
  if (rc) return 0;
  if (pCur->pKeyInfo) {
    lumoIndexRecord(lumoCursorSpills(pCur), &key, &data, &rec, &nRec);
    return nRec;
  }
  return data.mv_size;
}

/* sqlite3VdbeMemFromBtree (vdbemem.c) reports SQLITE_CORRUPT when the
** requested window exceeds this bound.  2147483647, the largest record
** SQLite handles, never rejects a valid request; upstream returns a
** smaller bound only to limit allocations on a corrupt database. */
sqlite3_int64 sqlite3BtreeMaxRecordSize(BtCursor *pCur){
  return 2147483647;
}

#if SQLITE_VERSION_NUMBER < 3041000
/* Pre-3.41: returned char* with the error message in the return value,
 * no pzOut, no aCnt. */
char *sqlite3BtreeIntegrityCheck(sqlite3 *db, Btree *p, Pgno *aRoot, int nRoot, int mxErr, int *pnErr) {
  LUMO_LOG("called: sqlite3BtreeIntegrityCheck\n");
  return NULL;
}
#else
/* In SQLite 3.41 and later the error message is returned through *pzOut,
** and in 3.46 and later aCnt receives an entry count for each checked root.
** This backend leaves page structure to LMDB, so it reports zero errors, and
** it fills aCnt with each root's entry count. */
int sqlite3BtreeIntegrityCheck(
    sqlite3 *db, Btree *p, Pgno *aRoot,
#if SQLITE_VERSION_NUMBER >= 3046000
    sqlite3_value *aCnt,  /* OUT: per-root entry counts */
#endif
    int nRoot, int mxErr, int *pnErr, char **pzOut
) {
  LUMO_LOG("called: sqlite3BtreeIntegrityCheck (nRoot=%d)\n", nRoot);
  if (pzOut) *pzOut = NULL;
  /* LMDB owns page-level structural integrity, so we report no structural
  ** errors.  But PRAGMA integrity_check does NOT rely on our return value
  ** alone: after OP_IntegrityCk, SQLite emits VDBE code that compares each
  ** index's entry count against its table's, reading both from the aCnt
  ** register array that this function is required to populate (one int64
  ** per checked root, via sqlite3MemSetArrayInt64).  Leaving aCnt unset
  ** makes that comparison fire "wrong # of entries in index <name>" on
  ** every indexed table, a false positive on good data.
  **
  ** LMDB gives an exact per-dbi count via mdb_stat().ms_entries, so we
  ** fill aCnt for every checked root.  get_table(..., get_table_nocmp, ..)
  ** maps a root page to its dbi (mdb_stat is comparator-independent).  The
  ** i-th checked root is aRoot[i] for i in [0, nRoot).  PRAGMA
  ** integrity_check also looks up every table row in each index; PRAGMA
  ** quick_check omits that lookup. */
#if SQLITE_VERSION_NUMBER >= 3046000
  if (aCnt) {
    int i;
    for (i = 0; i < nRoot; i++) {
      MDB_dbi dbi;
      MDB_stat st;
      i64 n = 0;
      unsigned int root = (unsigned int)aRoot[i];
      if (root != 0) {
        int rc2 = get_table(p, root, get_table_nocmp, &dbi);
        if (rc2 == 0 && mdb_stat(p->svp->txn, dbi, &st) == 0) {
          n = (i64)st.ms_entries;
        }
      }
      sqlite3MemSetArrayInt64(aCnt, i, n);
    }
  }
#endif
  if (pnErr) *pnErr = 0;
  return SQLITE_OK;
}
#endif

Pager *sqlite3BtreePager(Btree *p){
  return (Pager *)p;
}

/* The Pager handle that sqlite3BtreePager() returns is the Btree, so these
** pager functions are defined here, where Btree is complete.
** SQLITE_FCNTL_DATA_VERSION reports PRAGMA data_version plus the number of
** write transactions this connection has committed, so it changes on every
** commit to the database, including this connection's own. */
u32 sqlite3PagerDataVersion(Pager *pPager){
  Btree *p = (Btree *)pPager;
  return (u32)p->iDataVersion + p->nOwnCommit;
}

/* OP_JournalMode is the caller, and it treats "" as a temporary database. */
#if SQLITE_VERSION_NUMBER >= 3031000
const char *sqlite3PagerFilename(const Pager *pPager, int nullIfMemDb){
#else
const char *sqlite3PagerFilename(Pager *pPager, int nullIfMemDb){
#endif
  (void)nullIfMemDb;
  return sqlite3BtreeGetFilename((Btree *)pPager);
}

/* sqlite3BtreeClearCache, introduced in SQLite 3.41.
 * Hints to the underlying pager to release cached pages.  LMDB does not
 * use the SQLite page cache, so this is a no-op. */
#if SQLITE_VERSION_NUMBER >= 3041000
void sqlite3BtreeClearCache(Btree *p) {
  LUMO_LOG("called: sqlite3BtreeClearCache (no-op for LMDB)\n");
  (void)p;
}
#endif

/* sqlite3BtreeIsEmpty, used by OP_IfEmpty in SQLite 3.51 and later.  Reports
** whether the cursor's table has no rows, from mdb_stat's entry count or,
** for an ephemeral table, the in-memory tree's count. */
#if SQLITE_VERSION_NUMBER >= 3051000
int sqlite3BtreeIsEmpty(BtCursor *pCur, int *pRes) {
  MDB_stat st;
  int rc;
  LUMO_LOG("called: sqlite3BtreeIsEmpty\n");
  if (pRes == NULL) return SQLITE_MISUSE;
  if (pCur && pCur->pBtree && pCur->pBtree->pEph) {
    *pRes = (ephCount((EphTree *)pCur->pBtree->pEph) == 0);
    return SQLITE_OK;
  }
  if (pCur == NULL || pCur->cursor == NULL) { *pRes = 1; return SQLITE_OK; }
  rc = mdb_stat(pCur->txn, pCur->dbi, &st);
  if (rc != 0) {
    LUMO_LOG("sqlite3BtreeIsEmpty: mdb_stat -> %d\n", rc);
    return error_map(rc);
  }
  *pRes = (st.ms_entries == 0);
  return SQLITE_OK;
}
#endif

/* Row count for OP_Count with P3 non-zero and for OP_IfSizeBetween, which
** asserts a positive count for a non-empty table.  mdb_stat's entry count is
** exact.  Returns -1 if mdb_stat fails. */
i64 sqlite3BtreeRowCountEst(BtCursor *pCur){
  MDB_stat st;
  int rc;
  if (pCur && pCur->pBtree && pCur->pBtree->pEph)
    return ephCount((EphTree *)pCur->pBtree->pEph);
  if (pCur == NULL || pCur->cursor == NULL) return 0;
  rc = mdb_stat(pCur->txn, pCur->dbi, &st);
  if (rc != 0) {
    LUMO_LOG("sqlite3BtreeRowCountEst: mdb_stat -> %d\n", rc);
    return -1;  /* "no estimate available" sentinel */
  }
  return (i64)st.ms_entries;
}

/* Incremental-blob read for sqlite3_blob_read().  A tripped cursor returns
** its tripCode and a cursor with no position SQLITE_ABORT, matching
** upstream's saved error for a faulted cursor and SQLITE_ABORT for an invalid
** one.  A cursor whose position was saved at a savepoint is restored.  The
** row is read from LMDB, not from the cursor's cached row, because a write
** through another cursor in the same transaction can move the bytes the
** cached pointer points to. */
int sqlite3BtreePayloadChecked(BtCursor *pCur, u32 offset, u32 amt, void *pBuf){
  MDB_val key, data;
  const unsigned char *src;
  u32 srcSize;
  int rc;
  LUMO_LOG("called: sqlite3BtreePayloadChecked offset=%u amt=%u\n", offset, amt);
  if (pCur == NULL) return SQLITE_ABORT;
  if (pCur->tripCode != SQLITE_OK) {
    return pCur->tripCode;
  }
  if (pCur->cursor == NULL && !pCur->savedKey.mv_data) {
    /* upstream: eState==CURSOR_INVALID -> SQLITE_ABORT */
    return SQLITE_ABORT;
  }
  if (pCur->pKeyInfo) {
    /* index cursor: payload is the key, except spilled WITHOUT ROWID records
    ** whose full payload is in the value.  Use the saved key when it is the
    ** whole payload so we don't accidentally step the cursor. */
    if (pCur->savedKey.mv_data && !lumoCursorSpills(pCur)) {
      key = pCur->savedKey;
      data.mv_data = NULL; data.mv_size = 0;
    } else {
      rc = restoreCursor(pCur, 1);
      if (rc != SQLITE_OK) return rc;
      rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
      if (rc) return SQLITE_IOERR_READ;
    }
    lumoIndexRecord(lumoCursorSpills(pCur), &key, &data, &src, &srcSize);
  } else {
    rc = restoreCursor(pCur, 1);
    if (rc != SQLITE_OK) return rc;
    rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
    if (rc) return SQLITE_IOERR_READ;
    src = (const unsigned char *)data.mv_data;
    srcSize = (u32)data.mv_size;
  }
  /* SQLite's callers keep the window within the payload, which upstream
  ** asserts; a window outside the payload is reported as corruption. */
  if ((u64)offset + (u64)amt > (u64)srcSize) {
    LUMO_LOG("sqlite3BtreePayloadChecked: out of range offset=%u amt=%u size=%u\n",
             offset, amt, srcSize);
    return SQLITE_CORRUPT_BKPT;
  }
  memcpy(pBuf, src + offset, amt);
  return SQLITE_OK;
}

/* sqlite3BtreePutData, incremental-blob write path.
**
** Used by sqlite3_blob_write() (vdbeblob.c).  Modifies a window of the
** data column of the row the cursor is currently positioned at,
** without changing the row's overall length.  Upstream restricts this
** to INTKEY (table) cursors opened for writing.  We do the same.
**
** LMDB has no native partial-record update, so this is a
** read-modify-write inside the cursor's existing write transaction:
**
**   1. Fetch the current (key, data) via MDB_GET_CURRENT.
**   2. Allocate a same-size buffer, copy the existing data into it.
**   3. Overlay pBuf at offset for amt bytes.
**   4. mdb_cursor_put(cursor, key, &newdata, MDB_CURRENT) to overwrite.
**
** MDB_CURRENT keys off the cursor's current position, so the cursor
** stays where it was. */
int sqlite3BtreePutData(BtCursor *pCsr, u32 offset, u32 amt, void *z){
  MDB_val key, data, newdata;
  unsigned char *buf;
  int rc;
  LUMO_LOG("called: sqlite3BtreePutData offset=%u amt=%u\n", offset, amt);
  if (pCsr == NULL) return SQLITE_ABORT;
  if (pCsr->tripCode != SQLITE_OK) return pCsr->tripCode;
  if (pCsr->cursor == NULL && !pCsr->savedKey.mv_data) return SQLITE_ABORT;
  if (pCsr->pKeyInfo) {
    /* upstream's sqlite3BtreePutData asserts pCsr->pPage->intKey;
    ** index cursors have no business here. */
    return SQLITE_INTERNAL;
  }
  if (! pCsr->wrFlag) {
    return SQLITE_READONLY;
  }
  /* Reposition the cursor if a savepoint or other operation moved
  ** state, then read the current row via MDB_GET_CURRENT. */
  rc = restoreCursor(pCsr, 1);
  if (rc != SQLITE_OK) return rc;
  rc = mdb_cursor_get(pCsr->cursor, &key, &data, MDB_GET_CURRENT);
  if (rc) return error_map(rc);
  /* sqlite3_blob_write() rejects a window past the row before calling here,
  ** and upstream asserts that the window is within the row; a window
  ** outside the row is reported as corruption. */
  if ((u64)offset + (u64)amt > (u64)data.mv_size) {
    LUMO_LOG("sqlite3BtreePutData: out of range offset=%u amt=%u size=%zu\n",
             offset, amt, (size_t)data.mv_size);
    return SQLITE_CORRUPT_BKPT;
  }
  /* Build the modified buffer.  We need a separate allocation because
  ** data.mv_data points into LMDB's mmap'd region and is read-only
  ** when the txn was opened r/o (and even when r/w, modifying it in
  ** place would skip the COW that mdb_cursor_put performs). */
  buf = sqlite3_malloc((int)data.mv_size);
  if (buf == NULL) return SQLITE_NOMEM_BKPT;
  memcpy(buf, data.mv_data, data.mv_size);
  memcpy(buf + offset, z, amt);
  newdata.mv_size = data.mv_size;
  newdata.mv_data = buf;
  rc = mdb_cursor_put(pCsr->cursor, &key, &newdata, MDB_CURRENT);
  sqlite3_free(buf);
  if (rc) {
    LUMO_LOG("sqlite3BtreePutData: mdb_cursor_put -> %d\n", rc);
    return error_map(rc);
  }
  cursorCacheClearAfterWrite(pCsr);
  return SQLITE_OK;
}

/* Mark pCur as the cursor of a blob handle open on its current row.
** Replacing or deleting that row, or clearing its table, trips the cursor
** with SQLITE_ABORT; see invalidateIncrblobCursors. */
void sqlite3BtreeIncrblobCursor(BtCursor *pCur){
  LUMO_LOG("called: sqlite3BtreeIncrblobCursor\n");
  pCur->isIncrblob = 1;
  pCur->incrblobRowid = sqlite3BtreeIntegerKey(pCur);
  pCur->pBtree->hasIncrblobCur = 1;
}

void sqlite3BtreeClearCursor(BtCursor *pCur){
  LUMO_LOG("called: sqlite3BtreeClearCursor\n");
  /* The cursor no longer has a current row.  Drop the cached row and mark it
  ** cleared so a following Next/Prev reports EOF rather than stepping from the
  ** LMDB position the cursor still physically holds. */
  cursorCacheClear(pCur);
  pCur->cleared = 1;
}

/* Upstream writes 1 or 2 to header bytes 18 and 19 to turn WAL off or on;
** OP_JournalMode and sqlite3_backup_step() call this function.  LMDB has no
** such header fields, so this function reports success. */
int sqlite3BtreeSetVersion(Btree *p, int iVersion) {
  LUMO_LOG("called: sqlite3BtreeSetVersion(%d)\n", iVersion);
  return SQLITE_OK;
}

/* Return true if the cursor has any hint in mask.  OP_SeekGE and OP_SeekLE
** use it to detect BTREE_SEEK_EQ. */
int sqlite3BtreeCursorHasHint(BtCursor *pCur, unsigned int mask) {
  return (pCur->hints & mask)!=0;
}

/* Return 1 if the LMDB environment was opened MDB_RDONLY (p->mdbFlags, set
** by sqlite3BtreeOpen), else 0.  SQLite drops CHECK constraints when it
** returns non-zero (sqlite3AddCheckConstraint), BEGIN IMMEDIATE or EXCLUSIVE
** starts only a read transaction on a read-only database
** (sqlite3BeginTransaction), and sqlite3_db_readonly() returns its value to
** the application. */
int sqlite3BtreeIsReadonly(Btree *p) {
  if (p == NULL) return 0;
  return (p->mdbFlags & MDB_RDONLY) ? 1 : 0;
}

/* sqlite3HeaderSizeBtree: in upstream, returns the size of the
** per-page header that the btree adds to each page in the SQLite
** pcache.  Used by sqlite3_config(SQLITE_CONFIG_PCACHE_HDRSZ, &n) to
** report the total per-page overhead clients need to budget when
** supplying their own page cache via SQLITE_CONFIG_PAGECACHE.  LMDB
** doesn't use the pcache, so we contribute zero bytes of header. */
int sqlite3HeaderSizeBtree(void) {
  return 0;
}

/* Return 1 if the cursor is on a row: it is not tripped, and it has an LMDB
** cursor that is not at EOF or, for an ephemeral table, a tree cursor on a
** node.  OP_SeekScan, OP_SeekEnd, sqlite3VdbeFindIndexKey (SQLite 3.53 and
** later), sqlite3_blob_write() with a preupdate hook (3.51 and later) and
** OP_Delete in SQLITE_DEBUG builds call it. */
int sqlite3BtreeCursorIsValidNN(BtCursor *pCur) {
  if (pCur && pCur->pBtree && pCur->pBtree->pEph)
    return (pCur->tripCode == SQLITE_OK
            && ((EphCursor *)pCur->ephCur)->cur != NULL);
  return (pCur != NULL
          && pCur->cursor != NULL
          && pCur->tripCode == SQLITE_OK
          && !pCur->atEof);
}

/*
** The first argument, pCur, is a cursor opened on some b-tree. Count the
** number of entries in the b-tree and write the result to *pnEntry.
**
** SQLITE_OK is returned if the operation is successfully executed. 
** Otherwise, if an error is encountered (i.e. an IO error or database
** corruption) an SQLite error code is returned.
*/
int sqlite3BtreeCount(sqlite3 *db, BtCursor *pCur, i64 *pnEntry) {
  MDB_stat st;
  int rc;
  rc = mdb_stat(pCur->txn, pCur->dbi, &st);
  LUMO_LOG("sqlite3BtreeCount db=%p pCur=%p => rc=%d num=%zd\n",
	   db, pCur, rc, (size_t)(rc ? 0 : st.ms_entries));
  if (rc) return error_map(rc);
  *pnEntry = (i64)st.ms_entries;
  return SQLITE_OK;
}

#ifndef SQLITE_OMIT_SHARED_CACHE
/* When SQLITE_OMIT_SHARED_CACHE is defined (e.g. Fossil's build), the
** SQLite headers turn sqlite3BtreeSharable / ConnectionCount / Enter /
** EnterAll / EnterCursor into no-op macros.  Defining them as functions
** as well then collides with the macro expansion ("expected identifier
** before '{'"), so these definitions are compiled only when shared cache
** is NOT omitted.  LMDB never uses shared cache, but the symbols must
** still exist for the non-omit build. */
int sqlite3BtreeSharable(Btree *p) {
  return 0;
}

/* sqlite3BtreeConnectionCount: in upstream, returns how many sqlite3
** handles share the same BtShared (relevant only for shared-cache
** mode, which LMDB doesn't implement, see sqlite3_enable_shared_cache
** earlier in this file: returns 0 unconditionally).
** sqlite3_db_status(SQLITE_DBSTATUS_CACHE_USED_SHARED) divides cache use by
** it.  Upstream
** even has `#define sqlite3BtreeConnectionCount(X) 1` in non-shared-
** cache builds, which is the canonical default. */
int sqlite3BtreeConnectionCount(Btree *p) {
  return 1;
}
#endif /* !SQLITE_OMIT_SHARED_CACHE */

/* sqlite3BtreeGetReserveNoMutex: in upstream, returns the per-page
** reserved-bytes count (= pageSize - usableSize) without taking the
** btree mutex.  Used by dbstat and the backup code.  LMDB has no
** per-page reserve.  Return 0. */
int sqlite3BtreeGetReserveNoMutex(Btree *p) {
  return 0;
}

/* Upstream returns the file offset of the current row's payload.
** OP_Column uses it to tell rows apart in its cache of large TEXT and
** BLOB values, reached only when sqlite3BtreePayloadFetch returned less
** than the whole row, and sqlite_offset() reports it.  LMDB rows have no
** file offset; the row's address distinguishes rows at any one time, and
** OP_Column discards the cache on every insert or delete, when rows can
** move. */
i64 sqlite3BtreeOffset(BtCursor *pCur) {
  MDB_val key, data;
  int rc;
  if (pCur == NULL || pCur->cursor == NULL) return 0;
  rc = mdb_cursor_get(pCur->cursor, &key, &data, MDB_GET_CURRENT);
  if (rc != 0) return 0;
  if (pCur->pKeyInfo) return (i64)(intptr_t)key.mv_data;
  return (i64)(intptr_t)data.mv_data;
}

/* This backend has no shared cache, so the mutex hooks do nothing.  btree.h
** declares the Enter family when shared cache is present, and the Leave family
** when shared cache is present and SQLITE_THREADSAFE is nonzero. */
#ifndef SQLITE_OMIT_SHARED_CACHE
void sqlite3BtreeEnter(Btree *p){ }
void sqlite3BtreeEnterCursor(BtCursor *pCur){ }
void sqlite3BtreeEnterAll(sqlite3 *db){ }
#endif

#if !defined(SQLITE_OMIT_SHARED_CACHE) && SQLITE_THREADSAFE
void sqlite3BtreeLeave(Btree *p){ }
void sqlite3BtreeLeaveCursor(BtCursor *pCur){ }
void sqlite3BtreeLeaveAll(sqlite3 *db){ }
#endif

/*
** Return true for an in-memory database (Btree.isMemdb, set by
** sqlite3BtreeOpen).  Unlike upstream, a temp database stored in a file
** returns false.
*/
/* this is here because we have a "fake" pager which is actually a Btree */
int sqlite3PagerIsMemdb(Pager *pPager){
  Btree *p = (Btree*)pPager;
  return p->isMemdb;
}

#endif /* __LUMO_BACKEND_btree_c */
