/*

  Copyright 2021 The LumoSQL Authors

  SPDX-License-Identifier: MIT
  SPDX-FileCopyrightText: 2021 The LumoSQL Authors
  SPDX-ArtifactOfProjectName: LumoSQL
  SPDX-FileType: Code
  SPDX-FileComment: Original by Claudio Calvelli, 2021

  not-fork.d/lmdb/files/backup.c

  LMDB-native implementation of SQLite's sqlite3_backup_* API for when the
  LMDB backend has replaced src/backup.c.

  LMDB has no page-incremental copy, but mdb_env_copy() snapshots a whole
  env consistently in a single call.  We therefore make backup single-shot:
  the first sqlite3_backup_step() performs the entire copy and returns
  SQLITE_DONE; remaining()/pagecount() report a one-page job so the shell's
  progress loop terminates cleanly.

*/

#ifndef __LUMO_BACKEND_backup_c
#define __LUMO_BACKEND_backup_c 1

/* The source is the Btree environment for (pSrcDb, zSrcDb). The destination
** is the Btree path for (pDestDb, zDestDb), which the caller has
** already opened as a real LMDB env.  Backup_step snapshots the source to a
** temporary sibling, replaces the destination, reopens it, and resets the
** destination connection's schema so it re-reads the copied content. */
struct sqlite3_backup {
  sqlite3 *pDestDb;
  sqlite3 *pSrcDb;
  Btree *pSrc;
  Btree *pDest;
  int done;        /* set once the single-shot copy has run */
  int rc;          /* result of the copy */
};

sqlite3_backup *sqlite3_backup_init(
  sqlite3* pDestDb,
  const char *zDestDb,
  sqlite3* pSrcDb,
  const char *zSrcDb
){
  sqlite3_backup *p;
  int iSrc, iDest;
  if (!pDestDb || !pSrcDb) return NULL;
  p = (sqlite3_backup*)sqlite3_malloc(sizeof(*p));
  if (!p) { sqlite3Error(pDestDb, SQLITE_NOMEM); return NULL; }
  memset(p, 0, sizeof(*p));
  p->pDestDb = pDestDb;
  p->pSrcDb  = pSrcDb;
  iSrc  = sqlite3FindDbName(pSrcDb,  zSrcDb  ? zSrcDb  : "main");
  iDest = sqlite3FindDbName(pDestDb, zDestDb ? zDestDb : "main");
  if (iSrc < 0 || iDest < 0) { sqlite3_free(p); return NULL; }
  p->pSrc  = pSrcDb->aDb[iSrc].pBt;
  p->pDest = pDestDb->aDb[iDest].pBt;
  return p;
}

int sqlite3_backup_step(sqlite3_backup *p, int nPage){
  int rc;
  Btree *pSrc, *pDest;
  MDB_envinfo dInfo; size_t dMap = 0;
  char tmp[BTREE_MAX_PATH + 8];
  (void)nPage;
  if (!p) return SQLITE_MISUSE;
  if (p->done) return SQLITE_DONE;     /* single-shot: already copied */

  pSrc = p->pSrc; pDest = p->pDest;
  if (!pSrc || !pDest || !pSrc->env) { p->rc = SQLITE_INTERNAL; p->done = 1; return SQLITE_ERROR; }
  if (pSrc->isMemdb || pDest->isMemdb) { p->rc = SQLITE_INTERNAL; p->done = 1; return SQLITE_ERROR; }

  if (mdb_env_info(pDest->env, &dInfo) == 0) dMap = (size_t)dInfo.me_mapsize;
  sqlite3_snprintf(sizeof tmp, tmp, "%s-backup", pDest->path);
  unlink(tmp);

  /* mdb_env_copy starts its own read transaction.  MDB_NOTLS lets it use a
  ** separate reader slot when this thread already holds the source reader. */
  rc = mdb_env_copy(pSrc->env, tmp);
  if (rc) goto done;

  /* The complete copy now exists, so close the destination before replacing
  ** its data file. */
  invalidateCursors(pDest, pDest->svp, SQLITE_ABORT, 0);
  if (pDest->svp) {
    BtreeSavepoint *S = pDest->svp, *P;
    while (S) { P = S->prev; if (S->txn) mdb_txn_abort(S->txn); sqlite3_free(S); S = P; }
    pDest->svp = NULL;
  }
  pDest->inTrans = SQLITE_TXN_NONE;
  pDest->hasData = 0;
  mdb_env_close(pDest->env);
  pDest->env = NULL;

  if (rename(tmp, pDest->path) != 0) {
    rc = errno;
  } else {
    char fn[BTREE_MAX_PATH + 8];
    sqlite3_snprintf(sizeof fn, fn, "%s-lock", pDest->path);
    unlink(fn);
  }

  /* Reopen either the copied destination or, if rename failed, the original. */
  {
    int rc2 = mdb_env_create(&pDest->env);
    if (rc2 == 0) {
      mdb_env_set_maxdbs(pDest->env, 1024);
      if (dMap) mdb_env_set_mapsize(pDest->env, dMap);
      mdb_env_set_maxreaders(pDest->env, 254);
      rc2 = mdb_env_open(pDest->env, pDest->path, pDest->mdbFlags, SQLITE_DEFAULT_FILE_PERMISSIONS);
    }
    if (rc2 && rc == 0) rc = rc2;
  }

  if (rc == 0) sqlite3ResetAllSchemasOfConnection(p->pDestDb);

done:
  if (rc) unlink(tmp);
  p->rc = error_map(rc);
  p->done = 1;
  return (rc == 0) ? SQLITE_DONE : SQLITE_ERROR;
}

int sqlite3_backup_pagecount(sqlite3_backup *p){ (void)p; return 1; }
int sqlite3_backup_remaining(sqlite3_backup *p){ return (p && p->done) ? 0 : 1; }

int sqlite3_backup_finish(sqlite3_backup *p){
  int rc;
  if (!p) return SQLITE_OK;
  rc = p->rc;
  sqlite3_free(p);
  return rc;
}

#endif
