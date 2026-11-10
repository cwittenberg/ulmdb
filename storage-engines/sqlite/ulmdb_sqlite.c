/* Optional SQLite tables through the cooperative C ABI or owned native bridge. */
#include <sqlite3ext.h>
SQLITE_EXTENSION_INIT1
#ifdef ULMDB_SQLITE_OWNED
#include "native/owned_backend.h"
#define ULMDB_MODULE "ulmdb_owned"
#define ULMDB_ENTRY sqlite3_ulmdbsqliteowned_init
#else
#include "lmdb.h"
#define ULMDB_MODULE "ulmdb"
#define ULMDB_ENTRY sqlite3_ulmdbsqlite_init
#endif
#include <stdint.h>
#include <string.h>
#include <limits.h>
#include <stdlib.h>
#include <errno.h>

#define MAX_BATCH_KEYS 65536
#define MAX_BATCH_BYTES (UINT64_C(64) * 1024 * 1024)

typedef struct { int id; MDB_txn *txn; } Save;
typedef struct Cursor Cursor;
typedef struct {
    int refs;
    char *path, *schema, *table;
    MDB_env *env;
    MDB_dbi dbi;
    MDB_txn *write;
    Save *saves;
    int nsaves, capacity;
    unsigned epoch;
#ifdef ULMDB_SQLITE_OWNED
    Cursor *idle_cursor;
    MDB_txn *idle_reader;
    Cursor *cursors;
#endif
} Module;
typedef struct {
    sqlite3_vtab base;
    sqlite3 *db;
    Module *module;
} Table;
typedef struct {
    sqlite3_int64 id;
    void *value;
    int bytes;
} BatchRow;
struct Cursor {
    sqlite3_vtab_cursor base;
    MDB_txn *read;
    MDB_cursor *cursor;
    sqlite3_int64 id;
    void *value;
    int bytes, eof, exact, writer;
#ifdef ULMDB_SQLITE_OWNED
    sqlite3_int64 *keys;
    sqlite3_uint64 batch_bytes;
    int borrowed;
    Cursor *previous, *following;
#else
    BatchRow *rows;
#endif
    int batch, nrows, row;
    unsigned epoch;
};

static int error(Table *t, int rc, const char *operation) {
    sqlite3_free(t->base.zErrMsg);
    t->base.zErrMsg = sqlite3_mprintf("ULMDB %s: %s (%d)", operation, mdb_strerror(rc), rc);
    if (rc == MDB_KEYEXIST) return SQLITE_CONSTRAINT_PRIMARYKEY;
    if (rc == MDB_MAP_FULL || rc == MDB_TXN_FULL) return SQLITE_FULL;
    if (rc == MDB_READERS_FULL || rc == MDB_BAD_TXN || rc == EBUSY || rc == EAGAIN) return SQLITE_BUSY;
    return SQLITE_IOERR;
}
static int message(Table *t, int rc, const char *text) {
    sqlite3_free(t->base.zErrMsg);
    t->base.zErrMsg = sqlite3_mprintf("%s", text);
    return rc;
}
static void encode(sqlite3_int64 id, unsigned char key[8]) {
    uint64_t n = ((uint64_t)id) ^ (UINT64_C(1) << 63);
#if defined(__BYTE_ORDER__) && __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__ && (defined(__GNUC__) || defined(__clang__))
    n = __builtin_bswap64(n);
    memcpy(key, &n, sizeof(n));
#elif defined(__BYTE_ORDER__) && __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
    memcpy(key, &n, sizeof(n));
#else
    int i;
    for (i = 7; i >= 0; --i) { key[i] = (unsigned char)n; n >>= 8; }
#endif
}
static sqlite3_int64 decode(const void *key) {
#if defined(__BYTE_ORDER__) && __BYTE_ORDER__ == __ORDER_LITTLE_ENDIAN__ && (defined(__GNUC__) || defined(__clang__))
    uint64_t n;
    memcpy(&n, key, sizeof(n));
    n = __builtin_bswap64(n);
#elif defined(__BYTE_ORDER__) && __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
    uint64_t n;
    memcpy(&n, key, sizeof(n));
#else
    const unsigned char *p = key;
    uint64_t n = 0;
    int i;
    for (i = 0; i < 8; ++i) n = (n << 8) | p[i];
#endif
    n ^= UINT64_C(1) << 63;
    if (n <= INT64_MAX) return (sqlite3_int64)n;
    return -1 - (sqlite3_int64)(UINT64_MAX - n);
}
static MDB_txn *current(Table *t) {
    return t->module->nsaves ? t->module->saves[t->module->nsaves - 1].txn : t->module->write;
}
#ifdef ULMDB_SQLITE_OWNED
static int integer_key(sqlite3_value *value, sqlite3_int64 *key) {
    int type = sqlite3_value_numeric_type(value);
    if (type == SQLITE_INTEGER) {
        *key = sqlite3_value_int64(value);
        return 1;
    }
    if (type == SQLITE_FLOAT) {
        double real = sqlite3_value_double(value);
        /* Test bounds before conversion: (double)INT64_MAX rounds to 2^63.
         * The comparisons also reject infinities and NaNs without libm. */
        if (!(real >= -9223372036854775808.0 && real < 9223372036854775808.0)) return 0;
        *key = (sqlite3_int64)real;
        return (double)*key == real;
    }
    return 0;
}
#endif
static char *path_argument(const char *arg) {
    size_t n = strlen(arg), i, j = 0;
    char *out = sqlite3_malloc64(n + 1);
    char quote = n >= 2 && (arg[0] == '\'' || arg[0] == '"') && arg[n - 1] == arg[0] ? arg[0] : 0;
    if (!out) return 0;
    for (i = quote ? 1 : 0; i < n - (quote ? 1 : 0); ++i) {
        out[j++] = arg[i];
        if (quote && arg[i] == quote && arg[i + 1] == quote) ++i;
    }
    out[j] = 0;
    return out;
}
static int connect_table(sqlite3 *db, void *aux, int argc, const char *const *argv,
                         sqlite3_vtab **out, char **err, int create) {
    Module *module = aux;
    Table *t;
    MDB_txn *txn = 0;
    char *path;
    int rc;
    if (argc != 4) { *err = sqlite3_mprintf("usage: USING %s('/absolute/environment/directory')", ULMDB_MODULE); return SQLITE_ERROR; }
    if (module->refs && (strcmp(module->schema, argv[1]) || strcmp(module->table, argv[2]))) {
        *err = sqlite3_mprintf("only one ULMDB virtual table may be attached per SQLite connection");
        return SQLITE_ERROR;
    }
    path = path_argument(argv[3]);
    if (!path) return SQLITE_NOMEM;
    if (path[0] != '/') { sqlite3_free(path); *err = sqlite3_mprintf("ULMDB requires an absolute directory path"); return SQLITE_ERROR; }
    t = sqlite3_malloc64(sizeof(*t));
    if (!t) { sqlite3_free(path); return SQLITE_NOMEM; }
    memset(t, 0, sizeof(*t));
    t->db = db; t->module = module;
    if (module->refs && strcmp(module->path, path)) {
        *err = sqlite3_mprintf("ULMDB table path changed while an earlier statement remains attached");
        sqlite3_free(path); sqlite3_free(t); return SQLITE_BUSY;
    }
    rc = 0;
    if (!module->refs) {
        rc = mdb_env_create(&module->env);
        if (!rc) rc = mdb_env_set_maxdbs(module->env, 4);
        if (!rc) rc = mdb_env_open(module->env, path, MDB_NOTLS, 0600);
        if (!rc) rc = mdb_txn_begin(module->env, 0, create ? 0 : MDB_RDONLY, &txn);
        if (!rc) rc = mdb_dbi_open(txn, "sql_kv_v1", create ? MDB_CREATE : 0, &module->dbi);
        if (txn) {
            if (!rc && create) { rc = mdb_txn_commit(txn); txn = 0; }
            else { mdb_txn_abort(txn); txn = 0; }
        }
        if (rc) {
            *err = sqlite3_mprintf("ULMDB open: %s (%d); parent directory must exist", mdb_strerror(rc), rc);
            if (module->env) mdb_env_close(module->env);
            module->env = 0;
            sqlite3_free(path); sqlite3_free(t); return SQLITE_CANTOPEN;
        }
    }
    rc = sqlite3_declare_vtab(db, "CREATE TABLE x(id INTEGER PRIMARY KEY, value BLOB NOT NULL)");
    if (!rc) rc = sqlite3_vtab_config(db, SQLITE_VTAB_DIRECTONLY);
    if (!rc) rc = sqlite3_vtab_config(db, SQLITE_VTAB_CONSTRAINT_SUPPORT, 1);
    if (rc) {
        if (!module->refs) { mdb_env_close(module->env); module->env = 0; }
        sqlite3_free(path); sqlite3_free(t); return rc;
    }
    if (!module->refs) {
        module->schema = sqlite3_mprintf("%s", argv[1]);
        module->table = sqlite3_mprintf("%s", argv[2]);
        module->path = path; path = 0;
        if (!module->schema || !module->table) {
            sqlite3_free(module->schema); sqlite3_free(module->table); sqlite3_free(module->path);
            module->schema = module->table = module->path = 0;
            mdb_env_close(module->env); module->env = 0;
            sqlite3_free(t); return SQLITE_NOMEM;
        }
    }
    sqlite3_free(path);
    ++module->refs;
    *out = &t->base;
    return SQLITE_OK;
}
static int create_table(sqlite3 *db, void *aux, int n, const char *const *args,
                        sqlite3_vtab **out, char **err) {
    return connect_table(db, aux, n, args, out, err, 1);
}
static int reconnect_table(sqlite3 *db, void *aux, int n, const char *const *args,
                           sqlite3_vtab **out, char **err) {
    return connect_table(db, aux, n, args, out, err, 0);
}
static int rollback(sqlite3_vtab *base) {
    Table *t = (Table *)base;
#ifdef ULMDB_SQLITE_OWNED
    Cursor *c;
    /* Rollback cannot fail to allocate a saved row. Drop borrowed writer
     * pointers before abort frees their backing delta; the epoch rejects any
     * subsequent access through an outstanding writer cursor. */
    for (c = t->module->cursors; c; c = c->following) {
        if (c->writer && c->borrowed) {
            c->value = 0; c->bytes = 0; c->borrowed = 0;
        }
    }
#endif
    if (t->module->write) mdb_txn_abort(t->module->write); /* recursively aborts nested children */
    t->module->write = 0; t->module->nsaves = 0; ++t->module->epoch;
    return SQLITE_OK;
}
static int disconnect_table(sqlite3_vtab *base) {
    Table *t = (Table *)base;
    Module *m = t->module;
    if (--m->refs == 0) {
        rollback(base);
#ifdef ULMDB_SQLITE_OWNED
        sqlite3_free(m->idle_cursor); m->idle_cursor = 0;
        if (m->idle_reader) {
            if (usql_read_discard(m->idle_reader)) mdb_txn_abort(m->idle_reader);
            m->idle_reader = 0;
        }
#endif
        mdb_dbi_close(m->env, m->dbi);
        mdb_env_close(m->env); m->env = 0;
        sqlite3_free(m->saves); m->saves = 0; m->capacity = 0;
        sqlite3_free(m->path); sqlite3_free(m->schema); sqlite3_free(m->table);
        m->path = m->schema = m->table = 0;
    }
    sqlite3_free(t);
    return SQLITE_OK;
}
static int best_index(sqlite3_vtab *base, sqlite3_index_info *p) {
    int eq = -1, lower = -1, i, batch;
    (void)base;
    for (i = 0; i < p->nConstraint; ++i) {
        const struct sqlite3_index_constraint *c = &p->aConstraint[i];
        if (!c->usable || (c->iColumn != 0 && c->iColumn != -1)) continue;
        if (c->op == SQLITE_INDEX_CONSTRAINT_EQ &&
            (eq < 0 || (sqlite3_vtab_in(p, eq, -1) && !sqlite3_vtab_in(p, i, -1)))) eq = i;
        if (c->op == SQLITE_INDEX_CONSTRAINT_GE || c->op == SQLITE_INDEX_CONSTRAINT_GT) lower = i;
    }
    batch = eq >= 0 && sqlite3_vtab_in(p, eq, -1);
    i = eq >= 0 ? eq : lower;
    if (i >= 0) {
        p->aConstraintUsage[i].argvIndex = 1;
#ifdef ULMDB_SQLITE_OWNED
        /* Exact EQ/IN uses SQLite's own numeric affinity in integer_key(). */
        p->aConstraintUsage[i].omit = eq >= 0;
#else
        /* SQLite rechecks types, NULL, and strict bounds. */
        p->aConstraintUsage[i].omit = 0;
#endif
        p->idxNum = batch ? 3 : eq >= 0 ? 1 : 2;
        if (batch) sqlite3_vtab_in(p, eq, 1);
    }
    p->idxStr = batch ? "in-batch" : eq >= 0 ? "point" : lower >= 0 ? "lower-bound" : "scan";
    p->estimatedCost = batch ? 100.0 : eq >= 0 ? 10.0 : lower >= 0 ? 100000.0 : 1000000.0;
    p->estimatedRows = batch ? 32 : eq >= 0 ? 1 : lower >= 0 ? 100000 : 1000000;
    if (eq >= 0 && !batch) p->idxFlags = SQLITE_INDEX_SCAN_UNIQUE;
    if (p->nOrderBy == 1 && !p->aOrderBy[0].desc &&
        (p->aOrderBy[0].iColumn == 0 || p->aOrderBy[0].iColumn == -1)) p->orderByConsumed = 1;
    return SQLITE_OK;
}
static int open_cursor(sqlite3_vtab *base, sqlite3_vtab_cursor **out) {
    Cursor *c;
#ifdef ULMDB_SQLITE_OWNED
    Module *m = ((Table *)base)->module;
    c = m->idle_cursor; m->idle_cursor = 0;
    if (!c) c = sqlite3_malloc64(sizeof(*c));
#else
    c = sqlite3_malloc64(sizeof(*c));
#endif
    if (!c) return SQLITE_NOMEM;
    memset(c, 0, sizeof(*c)); c->base.pVtab = base; c->eof = 1;
#ifdef ULMDB_SQLITE_OWNED
    c->following = m->cursors;
    if (m->cursors) m->cursors->previous = c;
    m->cursors = c;
#endif
    *out = &c->base;
    return SQLITE_OK;
}
static void clear_value(Cursor *c) {
#ifdef ULMDB_SQLITE_OWNED
    if (!c->borrowed && c->value) sqlite3_free(c->value);
    c->borrowed = 0;
#else
    sqlite3_free(c->value);
#endif
    c->value = 0; c->bytes = 0;
}
static int set_value(Cursor *c, MDB_val *value, sqlite3_int64 id) {
#ifdef ULMDB_SQLITE_OWNED
    /* Read snapshots retain their bytes. Writer rows are preserved below
     * before any operation that can replace or move staged values. */
    if (c->writer) clear_value(c);
    c->value = value->mv_size ? value->mv_data : (void *)""; c->borrowed = 1;
#else
    {
        void *copy = sqlite3_malloc64(value->mv_size ? value->mv_size : 1);
        if (!copy) return SQLITE_NOMEM;
        if (value->mv_size) memcpy(copy, value->mv_data, value->mv_size);
        clear_value(c); c->value = copy;
    }
#endif
    c->bytes = (int)value->mv_size; c->id = id;
    return SQLITE_OK;
}
#ifdef ULMDB_SQLITE_OWNED
static int preserve_writer_values(Table *t) {
    Cursor *c;
    for (c = t->module->cursors; c; c = c->following) {
        if (c->writer && c->borrowed) {
            void *copy = sqlite3_malloc64(c->bytes ? (sqlite3_uint64)c->bytes : 1);
            if (!copy) return SQLITE_NOMEM;
            if (c->bytes) memcpy(copy, c->value, (size_t)c->bytes);
            c->value = copy; c->borrowed = 0;
        }
    }
    return SQLITE_OK;
}
#endif
static void reset_cursor(Cursor *c) {
#ifndef ULMDB_SQLITE_OWNED
    int i;
#endif
    if (c->cursor) mdb_cursor_close(c->cursor);
    c->cursor = 0;
    clear_value(c);
#ifdef ULMDB_SQLITE_OWNED
    if (c->read) {
        Module *m = ((Table *)c->base.pVtab)->module;
        if (!m->idle_reader && usql_read_recycle(c->read) == 0) m->idle_reader = c->read;
        else mdb_txn_abort(c->read);
    }
    sqlite3_free(c->keys); c->keys = 0; c->batch_bytes = 0;
#else
    if (c->read) mdb_txn_abort(c->read);
    for (i = 0; i < c->nrows; ++i) sqlite3_free(c->rows[i].value);
    sqlite3_free(c->rows); c->rows = 0;
#endif
    c->read = 0;
    c->nrows = 0; c->row = 0;
}
static int close_cursor(sqlite3_vtab_cursor *base) {
    Cursor *c = (Cursor *)base;
#ifdef ULMDB_SQLITE_OWNED
    Module *m = ((Table *)base->pVtab)->module;
    reset_cursor(c);
    if (c->previous) c->previous->following = c->following;
    else m->cursors = c->following;
    if (c->following) c->following->previous = c->previous;
    c->previous = c->following = 0;
    /* reset_cursor dropped every storage capability. open_cursor initializes
     * scalar state on reuse; retain no table pointer or last business key. */
    c->base.pVtab = 0; c->id = 0;
    if (!m->idle_cursor) m->idle_cursor = c;
    else sqlite3_free(c);
#else
    reset_cursor(c); sqlite3_free(c); return SQLITE_OK;
#endif
    return SQLITE_OK;
}
static int fetch(Cursor *c, int first, sqlite3_int64 seek) {
    Table *t = (Table *)c->base.pVtab;
    MDB_cursor *cursor = c->cursor;
    unsigned char encoded[8];
    MDB_val key = {8, encoded}, value = {0, 0};
    MDB_cursor_op operation;
    int rc;
    if (c->writer) {
        if (!t->module->write || c->epoch != t->module->epoch) return message(t, SQLITE_ABORT, "ULMDB write cursor invalidated by transaction end");
        rc = mdb_cursor_open(current(t), t->module->dbi, &cursor);
        if (rc) return error(t, rc, "write cursor");
    }
    if (!first && c->writer) {
        if (c->id == INT64_MAX) { c->eof = 1; mdb_cursor_close(cursor); return SQLITE_OK; }
        seek = c->id + 1;
    }
    operation = first || c->writer ? MDB_SET_RANGE : MDB_NEXT;
#ifdef ULMDB_SQLITE_OWNED
    /* INT64_MIN is the first possible fixed-width signed SQL key. Keep the
     * native unbounded scan path, including its versioned value resolver. */
    if (first && seek == INT64_MIN) operation = MDB_FIRST;
    if (!c->writer) {
        int64_t id = (int64_t)seek;
        rc = usql_cursor_row(cursor, &id, &value, operation);
        c->eof = rc == MDB_NOTFOUND;
        if (rc) return c->eof ? SQLITE_OK : error(t, rc, "scan");
        /* The typed native row API checks key shape and value bounds. */
        return set_value(c, &value, (sqlite3_int64)id);
    }
#endif
    if (operation == MDB_SET_RANGE) encode(seek, encoded);
    rc = mdb_cursor_get(cursor, &key, &value, operation);
    if (!rc && (key.mv_size != 8 || value.mv_size > INT_MAX)) {
        if (c->writer) mdb_cursor_close(cursor);
        return message(t, SQLITE_CORRUPT, "ULMDB SQL table contains invalid key or oversized value");
    }
    if (!rc) {
        int copy_rc = set_value(c, &value, decode(key.mv_data));
        if (copy_rc) { if (c->writer) mdb_cursor_close(cursor); return copy_rc; }
    }
    c->eof = rc == MDB_NOTFOUND;
    if (c->writer) mdb_cursor_close(cursor);
    return rc && rc != MDB_NOTFOUND ? error(t, rc, "scan") : SQLITE_OK;
}
static int compare_keys(const void *a, const void *b) {
    sqlite3_int64 left = *(const sqlite3_int64 *)a, right = *(const sqlite3_int64 *)b;
    return (left > right) - (left < right);
}
static int point_filter(Cursor *c, sqlite3_int64 id) {
    Table *t = (Table *)c->base.pVtab;
    unsigned char encoded[8];
    MDB_val key = {8, encoded}, value = {0, 0};
    int rc;
    if (c->writer && (!t->module->write || c->epoch != t->module->epoch))
        return message(t, SQLITE_ABORT, "ULMDB write cursor invalidated by transaction end");
    encode(id, encoded);
    rc = mdb_get(c->writer ? current(t) : c->read, t->module->dbi, &key, &value);
    c->eof = rc == MDB_NOTFOUND;
    if (rc) return rc == MDB_NOTFOUND ? SQLITE_OK : error(t, rc, "get");
    if (value.mv_size > INT_MAX) return message(t, SQLITE_CORRUPT, "ULMDB SQL table contains oversized value");
    return set_value(c, &value, id);
}
#ifdef ULMDB_SQLITE_OWNED
static int next_batch(Cursor *c) {
    Table *t = (Table *)c->base.pVtab;
    while (c->row < c->nrows) {
        int rc = point_filter(c, c->keys[c->row++]);
        if (rc) return rc;
        if (c->eof) continue;
        c->batch_bytes += c->bytes ? (sqlite3_uint64)c->bytes : 1;
        if (c->batch_bytes > MAX_BATCH_BYTES)
            return message(t, SQLITE_TOOBIG, "ULMDB IN batch exceeds 64 MiB of values");
        return SQLITE_OK;
    }
    c->eof = 1;
    return SQLITE_OK;
}
#endif
static int batch_filter(Cursor *c, sqlite3_value *list) {
    Table *t = (Table *)c->base.pVtab;
    sqlite3_value *value = 0;
    sqlite3_int64 *keys = 0;
#ifndef ULMDB_SQLITE_OWNED
    sqlite3_uint64 bytes = 0;
#endif
    int rc, nkeys = 0, capacity = 0, seen = 0, sorted = 1, i;
    for (rc = sqlite3_vtab_in_first(list, &value); rc == SQLITE_OK && value;
         rc = sqlite3_vtab_in_next(list, &value)) {
        sqlite3_int64 key;
        if (++seen > MAX_BATCH_KEYS) {
            rc = message(t, SQLITE_TOOBIG, "ULMDB IN batch exceeds 65536 candidate keys");
            break;
        }
#ifdef ULMDB_SQLITE_OWNED
        if (!integer_key(value, &key)) continue;
#else
        if (sqlite3_value_type(value) == SQLITE_NULL) continue;
        key = sqlite3_value_int64(value);
#endif
        if (nkeys == capacity) {
            int next_capacity = capacity ? capacity * 2 : 32;
            sqlite3_int64 *next_keys = sqlite3_realloc64(keys,
                (sqlite3_uint64)next_capacity * sizeof(*keys));
            if (!next_keys) { rc = SQLITE_NOMEM; break; }
            keys = next_keys; capacity = next_capacity;
        }
        if (nkeys && key < keys[nkeys - 1]) sorted = 0;
        keys[nkeys++] = key;
    }
    if (rc != SQLITE_OK && rc != SQLITE_DONE) { sqlite3_free(keys); return rc; }
    if (!nkeys) { sqlite3_free(keys); c->eof = 1; return SQLITE_OK; }
    if (!sorted) qsort(keys, (size_t)nkeys, sizeof(*keys), compare_keys);
#ifdef ULMDB_SQLITE_OWNED
    c->nrows = 1;
    for (i = 1; i < nkeys; ++i)
        if (keys[i] != keys[c->nrows - 1]) keys[c->nrows++] = keys[i];
    c->keys = keys;
    return next_batch(c);
#else
    c->rows = sqlite3_malloc64((sqlite3_uint64)nkeys * sizeof(*c->rows));
    if (!c->rows) { sqlite3_free(keys); return SQLITE_NOMEM; }
    for (i = 0; i < nkeys; ++i) {
        unsigned char encoded[8];
        MDB_val key = {8, encoded}, result = {0, 0};
        BatchRow *row = &c->rows[c->nrows];
        if (i && keys[i] == keys[i - 1]) continue;
        encode(keys[i], encoded);
        rc = mdb_get(c->writer ? current(t) : c->read, t->module->dbi, &key, &result);
        if (rc == MDB_NOTFOUND) continue;
        if (rc) { rc = error(t, rc, "IN get"); break; }
        if (result.mv_size > INT_MAX) {
            rc = message(t, SQLITE_CORRUPT, "ULMDB SQL table contains oversized value"); break;
        }
        bytes += result.mv_size ? result.mv_size : 1;
        if (bytes > MAX_BATCH_BYTES) {
            rc = message(t, SQLITE_TOOBIG, "ULMDB IN batch exceeds 64 MiB of values"); break;
        }
        row->value = sqlite3_malloc64(result.mv_size ? result.mv_size : 1);
        if (!row->value) { rc = SQLITE_NOMEM; break; }
        if (result.mv_size) memcpy(row->value, result.mv_data, result.mv_size);
        row->bytes = (int)result.mv_size; row->id = keys[i]; ++c->nrows;
    }
    sqlite3_free(keys);
    if (rc && rc != MDB_NOTFOUND) return rc;
    c->eof = c->nrows == 0;
    if (!c->eof) c->id = c->rows[0].id;
    return SQLITE_OK;
#endif
}
static int filter(sqlite3_vtab_cursor *base, int plan, const char *str, int argc, sqlite3_value **args) {
    Cursor *c = (Cursor *)base;
    Table *t = (Table *)base->pVtab;
    int rc = 0;
    (void)str; (void)argc;
    reset_cursor(c);
    c->exact = plan == 1; c->batch = plan == 3;
    c->writer = t->module->write != 0; c->epoch = t->module->epoch; c->eof = 0;
#ifdef ULMDB_SQLITE_OWNED
    if (c->exact && !c->writer) {
        sqlite3_int64 key;
        MDB_val value = {0, 0};
        if (!integer_key(args[0], &key)) { c->eof = 1; return SQLITE_OK; }
        c->read = t->module->idle_reader; t->module->idle_reader = 0;
        rc = usql_read_point(t->module->env, &c->read, (int64_t)key, &value);
        if (rc && rc != MDB_NOTFOUND) {
            if (c->read) {
                if (usql_read_discard(c->read)) mdb_txn_abort(c->read);
                c->read = 0;
            }
            return error(t, rc, "point read");
        }
        c->eof = rc == MDB_NOTFOUND;
        return c->eof ? SQLITE_OK : set_value(c, &value, key);
    }
#endif
    if (!c->writer) {
#ifdef ULMDB_SQLITE_OWNED
        if (t->module->idle_reader) {
            c->read = t->module->idle_reader; t->module->idle_reader = 0;
            rc = usql_read_renew(t->module->env, c->read);
            if (rc) {
                if (usql_read_discard(c->read)) mdb_txn_abort(c->read);
                c->read = 0;
            }
        } else
#endif
        rc = mdb_txn_begin(t->module->env, 0, MDB_RDONLY, &c->read);
        if (!rc && !c->exact && !c->batch) rc = mdb_cursor_open(c->read, t->module->dbi, &c->cursor);
    }
    if (rc) return error(t, rc, "read cursor");
    if (c->exact || c->batch) {
#ifdef ULMDB_SQLITE_OWNED
        sqlite3_int64 key;
        if (c->batch) rc = batch_filter(c, args[0]);
        else if (integer_key(args[0], &key)) rc = point_filter(c, key);
        else { c->eof = 1; rc = SQLITE_OK; }
#else
        rc = c->batch ? batch_filter(c, args[0]) : point_filter(c, sqlite3_value_int64(args[0]));
        /* All returned bytes are owned copies; no snapshot survives the batch. */
        if (c->read) { mdb_txn_abort(c->read); c->read = 0; }
#endif
        return rc;
    }
    /* A textual numeric prefix is not SQLite numeric affinity (for example
     * "3e-2"). Only actual INTEGER bounds can safely skip earlier SQL keys. */
    return fetch(c, 1, plan == 2 && sqlite3_value_type(args[0]) == SQLITE_INTEGER
        ? sqlite3_value_int64(args[0]) : INT64_MIN);
}
static int next(sqlite3_vtab_cursor *base) {
    Cursor *c = (Cursor *)base;
    if (c->exact) { c->eof = 1; return SQLITE_OK; }
    if (c->batch) {
#ifdef ULMDB_SQLITE_OWNED
        return next_batch(c);
#else
        c->eof = ++c->row >= c->nrows;
        if (!c->eof) c->id = c->rows[c->row].id;
        return SQLITE_OK;
#endif
    }
    return fetch(c, 0, 0);
}
static int eof(sqlite3_vtab_cursor *base) { return ((Cursor *)base)->eof; }
static int column(sqlite3_vtab_cursor *base, sqlite3_context *ctx, int col) {
    Cursor *c = (Cursor *)base;
    Table *t = (Table *)base->pVtab;
    if (c->writer && (!t->module->write || c->epoch != t->module->epoch))
        return message(t, SQLITE_ABORT, "ULMDB write cursor invalidated by transaction end");
    if (c->writer && sqlite3_vtab_nochange(ctx)) return SQLITE_OK;
    if (col == 0) sqlite3_result_int64(ctx, c->id);
#ifndef ULMDB_SQLITE_OWNED
    else if (c->batch) sqlite3_result_blob(ctx, c->rows[c->row].value, c->rows[c->row].bytes, SQLITE_TRANSIENT);
#endif
    else sqlite3_result_blob(ctx, c->value, c->bytes, SQLITE_TRANSIENT);
    return SQLITE_OK;
}
static int rowid(sqlite3_vtab_cursor *base, sqlite3_int64 *id) { *id = ((Cursor *)base)->id; return SQLITE_OK; }
static int begin(sqlite3_vtab *base) {
    Table *t = (Table *)base;
    int rc;
    if (t->module->write) return SQLITE_OK;
    rc = mdb_txn_begin(t->module->env, 0, 0, &t->module->write);
    return rc ? error(t, rc, "begin") : SQLITE_OK;
}
static int update(sqlite3_vtab *base, int argc, sqlite3_value **args, sqlite3_int64 *id) {
    Table *t = (Table *)base;
    unsigned char old_key[8], new_key[8];
    MDB_val key, value;
#ifndef ULMDB_SQLITE_OWNED
    MDB_val existing;
#endif
    sqlite3_int64 old_id = 0, new_id;
    int rc, deleting = argc == 1, inserting = sqlite3_value_type(args[0]) == SQLITE_NULL;
    int unchanged_value;
    if (!t->module->write) return message(t, SQLITE_MISUSE, "ULMDB update without transaction");
    if (!inserting) old_id = sqlite3_value_int64(args[0]);
    if (deleting) {
#ifdef ULMDB_SQLITE_OWNED
        rc = preserve_writer_values(t); if (rc) return rc;
#endif
        encode(old_id, old_key); key.mv_size = 8; key.mv_data = old_key;
        rc = mdb_del(current(t), t->module->dbi, &key, 0);
        return rc && rc != MDB_NOTFOUND ? error(t, rc, "delete") : SQLITE_OK;
    }
    if (!inserting && sqlite3_value_nochange(args[2])) new_id = old_id;
    else if (sqlite3_value_type(args[2]) != SQLITE_INTEGER)
        return message(t, SQLITE_CONSTRAINT_PRIMARYKEY, "ULMDB id must be an explicit signed 64-bit INTEGER");
    else new_id = sqlite3_value_int64(args[2]);
    if (sqlite3_value_type(args[1]) != SQLITE_NULL && sqlite3_value_int64(args[1]) != (inserting ? new_id : old_id))
        return message(t, SQLITE_CONSTRAINT_PRIMARYKEY, "set id to change the key; explicit rowid must agree with id");
    unchanged_value = !inserting && sqlite3_value_nochange(args[3]);
    if (!unchanged_value && sqlite3_value_type(args[3]) != SQLITE_BLOB)
        return message(t, SQLITE_CONSTRAINT_NOTNULL, "ULMDB value must be a non-NULL BLOB");
    encode(new_id, new_key); key.mv_size = 8; key.mv_data = new_key;
#ifndef ULMDB_SQLITE_OWNED
    if (inserting || old_id != new_id) {
        rc = mdb_get(current(t), t->module->dbi, &key, &existing);
        if (!rc) {
            int conflict = sqlite3_vtab_on_conflict(t->db);
            if (conflict == SQLITE_IGNORE) return SQLITE_CONSTRAINT_PRIMARYKEY;
            if (conflict != SQLITE_REPLACE) return error(t, MDB_KEYEXIST, "duplicate id");
        } else if (rc != MDB_NOTFOUND) return error(t, rc, "check id");
    }
#endif
    if (unchanged_value) {
        if (old_id == new_id) { *id = new_id; return SQLITE_OK; }
        encode(old_id, old_key);
        { MDB_val old = {8, old_key};
          rc = mdb_get(current(t), t->module->dbi, &old, &value); }
        if (rc) return error(t, rc, "unchanged value");
    } else {
        value.mv_size = (size_t)sqlite3_value_bytes(args[3]);
        value.mv_data = (void *)sqlite3_value_blob(args[3]);
    }
#ifdef ULMDB_SQLITE_OWNED
    rc = preserve_writer_values(t); if (rc) return rc;
    rc = mdb_put(current(t), t->module->dbi, &key, &value,
        inserting || old_id != new_id ? MDB_NOOVERWRITE : 0);
    if (rc == MDB_KEYEXIST) {
        int conflict = sqlite3_vtab_on_conflict(t->db);
        if (conflict == SQLITE_IGNORE) return SQLITE_CONSTRAINT_PRIMARYKEY;
        if (conflict == SQLITE_REPLACE) rc = mdb_put(current(t), t->module->dbi, &key, &value, 0);
    }
#else
    rc = mdb_put(current(t), t->module->dbi, &key, &value, 0);
#endif
    if (rc) return error(t, rc, "put");
    if (!inserting && old_id != new_id) {
        encode(old_id, old_key); key.mv_data = old_key;
        rc = mdb_del(current(t), t->module->dbi, &key, 0);
        if (rc && rc != MDB_NOTFOUND) return error(t, rc, "replace old id");
    }
    *id = new_id;
    return SQLITE_OK;
}
static int savepoint(sqlite3_vtab *base, int id) {
    Table *t = (Table *)base;
    MDB_txn *child = 0;
    int rc;
    if (!t->module->write) return SQLITE_OK;
    if (t->module->nsaves && t->module->saves[t->module->nsaves - 1].id == id) return SQLITE_OK;
#ifdef ULMDB_SQLITE_OWNED
    rc = preserve_writer_values(t); if (rc) return rc;
#endif
    if (t->module->nsaves == t->module->capacity) {
        int capacity = t->module->capacity ? t->module->capacity * 2 : 8;
        Save *saves;
        if (capacity > 1024) return message(t, SQLITE_TOOBIG, "ULMDB supports at most 1024 nested SQL savepoints");
        saves = sqlite3_realloc64(t->module->saves, (sqlite3_uint64)capacity * sizeof(*saves));
        if (!saves) return SQLITE_NOMEM;
        t->module->saves = saves; t->module->capacity = capacity;
    }
    rc = mdb_txn_begin(t->module->env, current(t), 0, &child);
    if (rc) return error(t, rc, "savepoint");
    t->module->saves[t->module->nsaves].id = id; t->module->saves[t->module->nsaves++].txn = child;
    return SQLITE_OK;
}
static int release(sqlite3_vtab *base, int id) {
    Table *t = (Table *)base;
#ifdef ULMDB_SQLITE_OWNED
    int rc = preserve_writer_values(t); if (rc) return rc;
#endif
    while (t->module->nsaves && t->module->saves[t->module->nsaves - 1].id >= id) {
        MDB_txn *child = t->module->saves[--t->module->nsaves].txn;
        int rc = mdb_txn_commit(child);
        if (rc) return error(t, rc, "release savepoint");
    }
    return SQLITE_OK;
}
static int rollback_to(sqlite3_vtab *base, int id) {
    Table *t = (Table *)base;
    int found = 0;
    if (!t->module->write) return SQLITE_OK;
#ifdef ULMDB_SQLITE_OWNED
    { int rc = preserve_writer_values(t); if (rc) return rc; }
#endif
    while (t->module->nsaves && t->module->saves[t->module->nsaves - 1].id >= id) {
        Save s = t->module->saves[--t->module->nsaves];
        found |= s.id == id;
        mdb_txn_abort(s.txn);
    }
    if (!found) {
        /* The SQL savepoint predates this table's first write. */
        rollback(base);
        { int rc = begin(base); if (rc) return rc; }
    }
    return savepoint(base, id);
}
/* SQLite propagates xSync errors but discards xCommit's return value. Publish
 * here so a failed ULMDB commit cannot appear as a successful SQL COMMIT.
 * This is an external-store commit boundary, not a cross-store two-phase commit.
 */
static int sync_table(sqlite3_vtab *base) {
    Table *t = (Table *)base;
    int rc = release(base, INT_MIN);
    MDB_txn *txn;
    if (rc) { rollback(base); return rc; }
    txn = t->module->write; t->module->write = 0; ++t->module->epoch;
    rc = txn ? mdb_txn_commit(txn) : 0;
    return rc ? error(t, rc, "commit") : SQLITE_OK;
}
static int commit(sqlite3_vtab *base) { (void)base; return SQLITE_OK; }
static const sqlite3_module module = {
    .iVersion = 2, .xCreate = create_table, .xConnect = reconnect_table,
    .xBestIndex = best_index, .xDisconnect = disconnect_table, .xDestroy = disconnect_table,
    .xOpen = open_cursor, .xClose = close_cursor, .xFilter = filter, .xNext = next,
    .xEof = eof, .xColumn = column, .xRowid = rowid, .xUpdate = update,
    .xBegin = begin, .xSync = sync_table, .xCommit = commit, .xRollback = rollback,
    .xSavepoint = savepoint, .xRelease = release, .xRollbackTo = rollback_to
};
#ifdef _WIN32
__declspec(dllexport)
#endif
int ULMDB_ENTRY(sqlite3 *db, char **err, const sqlite3_api_routines *api) {
    Module *state;
    int rc;
    SQLITE_EXTENSION_INIT2(api);
    if (sqlite3_libversion_number() < 3038000) {
        *err = sqlite3_mprintf("ULMDB virtual tables require SQLite 3.38 or newer");
        return SQLITE_ERROR;
    }
    state = sqlite3_malloc64(sizeof(*state));
    if (!state) return SQLITE_NOMEM;
    memset(state, 0, sizeof(*state));
    rc = sqlite3_create_module_v2(db, ULMDB_MODULE, &module, state, sqlite3_free);
    return rc;
}
