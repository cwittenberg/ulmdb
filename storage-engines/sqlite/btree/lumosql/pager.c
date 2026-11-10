/*

  Copyright 2021 The LumoSQL Authors under the terms contained in the file LICENSES/MIT

  SPDX-License-Identifier: MIT
  SPDX-FileCopyrightText: 2021 The LumoSQL Authors
  SPDX-ArtifactOfProjectName: LumoSQL
  SPDX-FileType: Code
  SPDX-FileComment: Original by Claudio Calvelli, 2021

  not-fork.d/lmdb/files/pager.c

  This file supplies the *minimum* set of pager.c symbols that upstream
  SQLite expects to find at link time when the LMDB backend has replaced
  src/pager.c.  These functions do not read or write pages: LMDB maps the
  database itself, commits by copy-on-write, and does its own locking, so it
  does not need an SQLite pager, page cache, journal or WAL.

  Each function does nothing or reports a fixed value, except the journal
  mode and journal size limit, which report the value last set by any
  connection in the process.

*/

#ifndef __LUMO_BACKEND_pager_c
#define __LUMO_BACKEND_pager_c 1

/* sqlite3PagerFile returns this zero-initialised placeholder.  Its NULL
** pMethods makes sqlite3OsFileControl return SQLITE_NOTFOUND and
** sqlite3OsFileControlHint do nothing. */
static sqlite3_file fakeFile = { 0, };

/* Upstream returns the main database's sqlite3_file for a journal or WAL
** filename that SQLite passed to xOpen.  The LMDB backend does not open a
** journal or WAL through a VFS, so there is no such file: return NULL. */
sqlite3_file *sqlite3_database_file_object(const char *zName){
  (void)zName;
  return NULL;
}

/* Upstream reports per-pcache statistics. report 0 for each stat. */
#if SQLITE_VERSION_NUMBER < 3045000
void sqlite3PagerCacheStat(Pager *pPager, int eStat, int reset, int *pnVal){
#else
void sqlite3PagerCacheStat(Pager *pPager, int eStat, int reset, u64 *pnVal){
#endif
  (void)pPager; (void)eStat; (void)reset;
  if (pnVal) *pnVal = 0;
}

/* Upstream returns the page-data buffer for a DbPage. This shouldn't be reached. */
void *sqlite3PagerGetData(DbPage *pPg){
  (void)pPg;
  return NULL;
}

/* Upstream closes the WAL connection.  LMDB has no WAL; nothing to do. */
int sqlite3PagerCloseWal(Pager *pPager, sqlite3 *db){
  (void)pPager; (void)db;
  return SQLITE_OK;
}

/* Upstream takes an EXCLUSIVE lock on the database file before committing
** a write transaction.  An LMDB write transaction already excludes other
** writers, so there is nothing to lock. */
int sqlite3PagerExclusiveLock(Pager *pPager){
  return SQLITE_OK;
}

sqlite3_file *sqlite3PagerFile(Pager *pPager){
  return &fakeFile;
}

/* Upstream walks the dirty list and writes pages out.  LMDB doesn't
** buffer writes through an SQLite pcache.  Commits go straight to mdb_*
** functions.  Nothing to flush. */
int sqlite3PagerFlush(Pager *pPager){
  (void)pPager;
  return SQLITE_OK;
}

/* Upstream loads a database page into the pcache.  LMDB pages are not
** available through this interface, so sqlite3_serialize, dbstat and
** sqlite_dbpage, which call it, get SQLITE_ERROR with *ppPage cleared. */
int sqlite3PagerGet(Pager *pPager, Pgno pgno, DbPage **ppPage, int flags) {
  (void)pPager; (void)pgno; (void)flags;
  if (ppPage) *ppPage = NULL;
  return SQLITE_ERROR;
}

/* Process-wide cache of the user's last-set journal mode.  LMDB has
** no SQLite-managed journal so the value has no effect.
** Initialised to PAGER_JOURNALMODE_OFF , the closest to
** "no journal" SQLite understands. */
static int cached_journal_mode = PAGER_JOURNALMODE_OFF;

int sqlite3PagerGetJournalMode(Pager *pPager){
  (void)pPager;
  return cached_journal_mode;
}

/* Upstream sets/queries the size limit for persistent journal files.
** LMDB has no journal.  Setting the limit has no effect, but for PRAGMA
** journal_size_limit to round-trip across set-then-query (the way
** vanilla SQLite would) we keep a process-wide cached value. */
i64 sqlite3PagerJournalSizeLimit(Pager *pPager, i64 iLimit){
  static i64 cached_limit = -1;  /* upstream default, SQLITE_DEFAULT_JOURNAL_SIZE_LIMIT */
  (void)pPager;
  if (iLimit >= -1) cached_limit = iLimit;
  return cached_limit;
}

/* Upstream returns the journal sqlite3_file*.  We have none. */
sqlite3_file *sqlite3PagerJrnlFile(Pager *pPager){
  (void)pPager;
  return NULL;
}

/* Upstream sets/queries the SQLite locking mode (NORMAL vs EXCLUSIVE).
** LMDB has its own MVCC locking and ignores this entirely.  Report
** PAGER_LOCKINGMODE_NORMAL as a stable default. */
int sqlite3PagerLockingMode(Pager *pPager, int eMode){
  (void)pPager; (void)eMode;
  return PAGER_LOCKINGMODE_NORMAL;
}

/* Upstream looks up a page in the pcache and returns NULL on miss.
** We have no pcache, so every lookup is a miss. */
DbPage *sqlite3PagerLookup(Pager *pPager, Pgno pgno){
  (void)pPager; (void)pgno;
  return NULL;
}

/* Upstream reports bytes of memory used by the pcache for this pager.
** LMDB doesn't allocate through the SQLite pcache. */
int sqlite3PagerMemUsed(Pager *pPager){
  (void)pPager;
  return 0;
}

/* Upstream returns true iff the pager state allows changing journal
** mode.  LMDB never holds journal-mode-blocking state, so always true. */
int sqlite3PagerOkToChangeJournalMode(Pager *pPager){
  (void)pPager;
  return 1;
}

/* Upstream returns the page count of the database via *pnPage.
** sqlite3BtreeLastPage already exposes this for LMDB.  If anything
** else asks via the pager API, return 0 rather than leave the output
** uninitialised. */
void sqlite3PagerPagecount(Pager *pPager, int *pnPage){
  (void)pPager;
  if (pnPage) *pnPage = 0;
}

/* Upstream sets the journal mode and returns the (possibly clamped)
** new value.  LMDB ignores journal mode but caches the value so a
** subsequent PRAGMA journal_mode query echoes back what the user set
** rather than always reporting "off".  See cached_journal_mode above. */
int sqlite3PagerSetJournalMode(Pager *pPager, int eMode){
  (void)pPager;
  cached_journal_mode = eMode;
  return eMode;
}

/* Upstream releases pcache pages that aren't currently pinned.  LMDB
** has no pcache.  Nothing to release. */
void sqlite3PagerShrink(Pager *pPager){
  (void)pPager;
}

/* Upstream decrements the refcount of a DbPage; we never produce
** DbPages, so this should not be reached.  No-op. */
void sqlite3PagerUnref(DbPage *pPg){
  (void)pPg;
}

void sqlite3PagerUnrefPageOne(DbPage *pPg){
  (void)pPg;
}

/* Upstream returns the VFS that owns the database file.  LMDB doesn't
** go through a SQLite VFS for its data.  We have no VFS to return. */
sqlite3_vfs *sqlite3PagerVfs(Pager *pPager){
  (void)pPager;
  return NULL;
}

int sqlite3PagerWalCallback(Pager *pPager){
  return SQLITE_OK;
}

/* no WAL needed for LMDB */
int sqlite3PagerWalSupported(Pager *pPager){
  return 0;
}

/* Upstream marks a page dirty so it gets written at commit.  LMDB
** writes go directly through mdb_put / mdb_cursor_put against an open
** transaction.  Nothing to mark dirty.  Return success so callers
** continue normally. */
int sqlite3PagerWrite(PgHdr *pPg){
  (void)pPg;
  return SQLITE_OK;
}

/* Upstream truncates the database image; the sqlite_dbpage virtual table
** calls this function in SQLite 3.47 and later.  LMDB has no pager-level
** truncate, so this function does nothing. */
void sqlite3PagerTruncateImage(Pager *pPager, Pgno nPage) {
  (void)pPager;
  (void)nPage;
}

#endif
