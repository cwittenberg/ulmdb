/* Optional SQL/MED interface. No PostgreSQL heap or ULMDB core changes. */
#include "postgres.h"

#include <stdint.h>
#include <limits.h>
#include <stdlib.h>
#include "access/htup_details.h"
#include "access/reloptions.h"
#include "access/xact.h"
#include "catalog/pg_foreign_server.h"
#include "catalog/pg_foreign_table.h"
#include "catalog/pg_operator_d.h"
#include "catalog/pg_type_d.h"
#include "commands/defrem.h"
#include "commands/explain.h"
#include "executor/executor.h"
#include "foreign/fdwapi.h"
#include "foreign/foreign.h"
#include "miscadmin.h"
#include "nodes/makefuncs.h"
#include "nodes/nodeFuncs.h"
#include "optimizer/appendinfo.h"
#include "optimizer/optimizer.h"
#include "optimizer/pathnode.h"
#include "optimizer/paths.h"
#include "optimizer/planmain.h"
#include "optimizer/restrictinfo.h"
#include "parser/parsetree.h"
#include "port/pg_bswap.h"
#include "utils/array.h"
#include "utils/builtins.h"
#include "utils/fmgroids.h"
#include "utils/inval.h"
#include "utils/lsyscache.h"
#include "utils/memutils.h"
#include "utils/rel.h"
#include "utils/syscache.h"
#include "ulmdb.h"
#ifdef ULMDB_FDW_SQL_BATCH
#include "native/sql_batch.h"
#define SQL_SCAN_BATCH 256
#endif

PG_MODULE_MAGIC;
PG_FUNCTION_INFO_V1(ulmdb_fdw_handler);
PG_FUNCTION_INFO_V1(ulmdb_fdw_validator);
void _PG_init(void);

typedef struct ULMDBScanState
{
    MDB_txn *txn;
    MDB_cursor *cursor;
    MDB_dbi dbi;
    bool missing_database;
    bool started;
    bool done;
    ExprState *key_expr;
    Oid key_type;
    bool batch;
    bool batch_evaluated;
    int64 *batch_keys;
    int batch_count;
    int batch_at;
    bool need_value;
    bytea *payload;
    Size payload_capacity;
#ifdef ULMDB_FDW_SQL_BATCH
    bool sql_batch_enabled;
    ulmdb_sql_row *sql_rows;
    ulmdb_sql_hit *sql_hits;
    size_t sql_count, sql_at;
    uint64 sql_refills;
#endif
    struct ULMDBScanState *next;
} ULMDBScanState;

typedef struct DatabaseState
{
    char *name;
    MDB_dbi read_dbi, write_dbi;
    bool read_checked, read_found, write_checked, write_found;
    struct DatabaseState *next;
} DatabaseState;

typedef struct ModifyState
{
    MDB_dbi dbi;
    AttrNumber id_attno;
} ModifyState;

typedef struct RelationState
{
    Oid relid;
    uint64 generation;
    char *canonical_path;
    char *database;
    struct RelationState *next;
} RelationState;

/* One environment per backend; one external snapshot per PG transaction. */
static MDB_env *cached_env;
static char *cached_path;
static MDB_txn *reader;
static MDB_txn *writer;
static ULMDBScanState *scans;
static DatabaseState *databases;
static RelationState *relations;
static uint64 metadata_generation = 1;

static void
check_mdb(int rc, const char *operation)
{
    if (rc != MDB_SUCCESS)
        ereport(ERROR,
                (errcode(ERRCODE_FDW_ERROR),
                 errmsg("ULMDB %s failed: %s", operation, mdb_strerror(rc))));
}

static void
require_access(void)
{
    if (!superuser())
        ereport(ERROR, (errcode(ERRCODE_INSUFFICIENT_PRIVILEGE),
                       errmsg("ulmdb_fdw currently requires a superuser")));
    if (XactIsoLevel != XACT_READ_COMMITTED)
        ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
                       errmsg("ulmdb_fdw supports READ COMMITTED isolation only")));
}

static void
close_scan(ULMDBScanState *state)
{
    if (state->cursor)
        mdb_cursor_close(state->cursor);
    state->cursor = NULL;
    state->txn = NULL;
}

static void
xact_callback(XactEvent event, void *arg)
{
    ULMDBScanState *s;
    (void) arg;
    if (event == XACT_EVENT_PRE_PREPARE && writer)
        ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
                       errmsg("ulmdb_fdw does not support prepared transactions")));
    if (event == XACT_EVENT_PRE_COMMIT || event == XACT_EVENT_ABORT ||
        event == XACT_EVENT_COMMIT || event == XACT_EVENT_PRE_PREPARE)
    {
        for (s = scans; s; s = s->next)
            close_scan(s);
        if (writer)
        {
            MDB_txn *txn = writer;
            writer = NULL;
            if (event == XACT_EVENT_PRE_COMMIT)
                check_mdb(mdb_txn_commit(txn), "commit");
            else
                mdb_txn_abort(txn);
        }
        if (reader)
        {
            mdb_txn_abort(reader);
            reader = NULL;
        }
        if (event != XACT_EVENT_PRE_COMMIT)
        {
            scans = NULL;
            databases = NULL;
            relations = NULL;
        }
    }
}

/* Invalidation callbacks hold no transaction-owned pointers. */
static void
relation_changed(Datum arg, Oid relid)
{
    (void) arg;
    (void) relid;
    if (++metadata_generation == 0) ++metadata_generation;
}

static void
options_changed(Datum arg, int cacheid, uint32 hashvalue)
{
    (void) arg;
    (void) cacheid;
    (void) hashvalue;
    if (++metadata_generation == 0) ++metadata_generation;
}

void
_PG_init(void)
{
    RegisterXactCallback(xact_callback, NULL);
    CacheRegisterRelcacheCallback(relation_changed, (Datum) 0);
    CacheRegisterSyscacheCallback(FOREIGNTABLEREL, options_changed, (Datum) 0);
    CacheRegisterSyscacheCallback(FOREIGNSERVEROID, options_changed, (Datum) 0);
}

static char *
option_value(List *options, const char *name)
{
    ListCell *lc;
    foreach(lc, options)
    {
        DefElem *def = lfirst_node(DefElem, lc);
        if (strcmp(def->defname, name) == 0)
            return defGetString(def);
    }
    return NULL;
}

Datum
ulmdb_fdw_validator(PG_FUNCTION_ARGS)
{
    List *options = untransformRelOptions(PG_GETARG_DATUM(0));
    Oid context = PG_GETARG_OID(1);
    ListCell *lc;
    bool seen = false;
    if (!superuser())
        ereport(ERROR, (errcode(ERRCODE_INSUFFICIENT_PRIVILEGE),
                       errmsg("only superusers may configure ulmdb_fdw")));
    foreach(lc, options)
    {
        DefElem *def = lfirst_node(DefElem, lc);
        const char *value = defGetString(def);
        bool valid = (context == ForeignServerRelationId &&
                      strcmp(def->defname, "path") == 0) ||
                     (context == ForeignTableRelationId &&
                      strcmp(def->defname, "database") == 0);
        if (!valid)
            ereport(ERROR, (errcode(ERRCODE_FDW_INVALID_OPTION_NAME),
                           errmsg("invalid ulmdb_fdw option \"%s\"", def->defname)));
        if (seen)
            ereport(ERROR, (errcode(ERRCODE_SYNTAX_ERROR),
                           errmsg("duplicate ulmdb_fdw option")));
        seen = true;
        if (!value[0] || (context == ForeignServerRelationId && value[0] != '/'))
            ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE),
                           errmsg("ULMDB path must be absolute and database must be nonempty")));
        if (context == ForeignTableRelationId && strlen(value) > 120)
            ereport(ERROR, (errcode(ERRCODE_INVALID_PARAMETER_VALUE),
                           errmsg("ULMDB database name exceeds 120 bytes")));
    }
    PG_RETURN_VOID();
}

static void
check_schema(Relation rel)
{
    TupleDesc desc = RelationGetDescr(rel);
    if (desc->natts != 2 || TupleDescAttr(desc, 0)->attisdropped ||
        TupleDescAttr(desc, 1)->attisdropped ||
        TupleDescAttr(desc, 0)->atttypid != INT8OID ||
        TupleDescAttr(desc, 1)->atttypid != BYTEAOID ||
        strcmp(NameStr(TupleDescAttr(desc, 0)->attname), "id") != 0 ||
        strcmp(NameStr(TupleDescAttr(desc, 1)->attname), "value") != 0 ||
        !TupleDescAttr(desc, 0)->attnotnull || !TupleDescAttr(desc, 1)->attnotnull)
        ereport(ERROR, (errcode(ERRCODE_FDW_INVALID_DATA_TYPE),
                       errmsg("ULMDB tables require exactly (id bigint NOT NULL, value bytea NOT NULL)")));
}

static MDB_env *
canonical_environment(const char *canonical)
{
    if (cached_env && strcmp(cached_path, canonical) != 0)
    {
        if (writer || reader || scans)
            ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
                           errmsg("one ULMDB environment per PostgreSQL transaction is supported")));
        mdb_env_close(cached_env);
        cached_env = NULL;
        pfree(cached_path);
        cached_path = NULL;
    }
    if (!cached_env)
    {
        MDB_env *env = NULL;
        int rc = mdb_env_create(&env);
        if (!rc) rc = mdb_env_set_maxdbs(env, 128);
        if (!rc) rc = mdb_env_set_mapsize(env, (size_t) 1024 * 1024 * 1024);
        if (!rc) rc = mdb_env_open(env, canonical, MDB_NOTLS, 0600);
        if (rc)
        {
            if (env) mdb_env_close(env);
            check_mdb(rc, "open environment");
        }
        cached_path = MemoryContextStrdup(TopMemoryContext, canonical);
        cached_env = env;
    }
    return cached_env;
}

static MDB_env *
get_environment(Relation rel, char **database)
{
    Oid relid = RelationGetRelid(rel);
    RelationState *state;
    ForeignTable *table;
    ForeignServer *server;
    char *path, *name;
    char canonical[PATH_MAX];
    MDB_env *env;
    require_access();
    for (state = relations; state; state = state->next)
        if (state->relid == relid) break;
    if (state && state->generation == metadata_generation)
    {
        *database = state->database;
        return canonical_environment(state->canonical_path);
    }
    check_schema(rel);
    table = GetForeignTable(relid);
    server = GetForeignServer(table->serverid);
    path = option_value(server->options, "path");
    name = option_value(table->options, "database");
    if (!path || !name)
        ereport(ERROR, (errcode(ERRCODE_FDW_OPTION_NAME_NOT_FOUND),
                       errmsg("ULMDB requires server option path and table option database")));
    if (!realpath(path, canonical))
        ereport(ERROR, (errcode_for_file_access(),
                       errmsg("ULMDB environment directory is unavailable: %m")));
    env = canonical_environment(canonical);
    if (!state)
    {
        state = MemoryContextAllocZero(TopTransactionContext, sizeof(*state));
        state->relid = relid;
        state->next = relations;
        relations = state;
    }
    if (state->canonical_path) pfree(state->canonical_path);
    if (state->database) pfree(state->database);
    state->canonical_path = MemoryContextStrdup(TopTransactionContext, canonical);
    state->database = MemoryContextStrdup(TopTransactionContext, name);
    state->generation = metadata_generation;
    *database = state->database;
    return env;
}

static MDB_txn *
get_writer(MDB_env *env)
{
    require_access();
    if (GetCurrentTransactionNestLevel() != 1)
        ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
                       errmsg("ULMDB writes inside savepoints or exception blocks are unsupported")));
    if (XactReadOnly)
        ereport(ERROR, (errcode(ERRCODE_READ_ONLY_SQL_TRANSACTION),
                       errmsg("cannot write ULMDB in a read-only transaction")));
    if (!writer)
    {
        check_mdb(mdb_txn_begin(env, NULL, 0, &writer), "begin write");
        if (reader)
        {
            unsigned char read_checkpoint[64];
            unsigned char writer_checkpoint[64];
            /* The writer lease fences concurrent commits before this check. */
            check_mdb(ulmdb_txn_base_checkpoint(reader, read_checkpoint), "read checkpoint");
            check_mdb(ulmdb_txn_base_checkpoint(writer, writer_checkpoint), "writer base checkpoint");
            if (memcmp(read_checkpoint, writer_checkpoint, sizeof(read_checkpoint)) != 0)
            {
                mdb_txn_abort(writer);
                writer = NULL;
                ereport(ERROR, (errcode(ERRCODE_T_R_SERIALIZATION_FAILURE),
                               errmsg("ULMDB changed since this transaction's first read"),
                               errhint("Retry the complete PostgreSQL transaction.")));
            }
        }
    }
    return writer;
}

static void
encode_id(int64 id, unsigned char bytes[8])
{
    uint64 sortable = pg_hton64(((uint64) id) ^ UINT64CONST(0x8000000000000000));
    memcpy(bytes, &sortable, sizeof(sortable));
}

static int64
decode_id(MDB_val *key)
{
    uint64 sortable;
    if (key->mv_size != 8)
        ereport(ERROR, (errcode(ERRCODE_DATA_CORRUPTED),
                       errmsg("ULMDB SQL table contains an invalid key")));
    memcpy(&sortable, key->mv_data, sizeof(sortable));
    return (int64) (pg_ntoh64(sortable) ^ UINT64CONST(0x8000000000000000));
}

/* DBI admission is stable within each real read/write transaction. */
static int
open_database(MDB_txn *txn, const char *name, bool create, MDB_dbi *dbi)
{
    DatabaseState *d;
    bool writing = txn == writer;
    bool *checked, *found;
    MDB_dbi *cached_dbi;
    int rc;
    for (d = databases; d; d = d->next)
        if (strcmp(d->name, name) == 0) break;
    if (!d)
    {
        d = MemoryContextAllocZero(TopTransactionContext, sizeof(*d));
        d->name = MemoryContextStrdup(TopTransactionContext, name);
        d->next = databases;
        databases = d;
    }
    checked = writing ? &d->write_checked : &d->read_checked;
    found = writing ? &d->write_found : &d->read_found;
    cached_dbi = writing ? &d->write_dbi : &d->read_dbi;
    if (*checked && (*found || !create))
    {
        if (*found) *dbi = *cached_dbi;
        return *found ? MDB_SUCCESS : MDB_NOTFOUND;
    }
    rc = mdb_dbi_open(txn, name, create ? MDB_CREATE : 0, cached_dbi);
    if (rc == MDB_SUCCESS)
    {
        unsigned int flags;
        check_mdb(mdb_dbi_flags(txn, *cached_dbi, &flags), "read database flags");
        if (flags != 0)
            ereport(ERROR,
                    (errcode(ERRCODE_FDW_INVALID_DATA_TYPE),
                     errmsg("ULMDB SQL tables require an ordinary bytewise KV database"),
                     errdetail("Database \"%s\" has incompatible flags 0x%x.", name, flags)));
#ifdef ULMDB_FDW_SQL_BATCH
        rc = ulmdb_sql_bytewise_format(txn, *cached_dbi);
        if (rc == MDB_INCOMPATIBLE)
            ereport(ERROR,
                    (errcode(ERRCODE_FDW_INVALID_DATA_TYPE),
                     errmsg("ULMDB SQL tables require an ordinary native bytewise KV database"),
                     errdetail("Database \"%s\" has an unsupported native format or comparator.", name),
                     errhint("Use a new SQL-created database, or migrate records into a native bytewise KV database.")));
        check_mdb(rc, "validate database format");
#endif
    }
    if (rc == MDB_SUCCESS || rc == MDB_NOTFOUND)
    {
        *checked = true;
        *found = rc == MDB_SUCCESS;
        if (*found) *dbi = *cached_dbi;
    }
    return rc;
}

static bool
integer_equality(Oid fn)
{
    return fn == F_INT8EQ || fn == F_INT84EQ || fn == F_INT82EQ ||
           fn == F_INT48EQ || fn == F_INT28EQ;
}

static bool
integer_array(Oid type)
{
    return type == INT2ARRAYOID || type == INT4ARRAYOID || type == INT8ARRAYOID;
}

/* Push builtin equality with constants/parameters, including equality ANY. */
static Expr *
lookup_expr(Expr *clause, Index relid)
{
    OpExpr *op;
    Expr *left, *right;
    Oid fn;
    Var *var;
    if (IsA(clause, ScalarArrayOpExpr))
    {
        ScalarArrayOpExpr *arrayop = (ScalarArrayOpExpr *) clause;
        ListCell *lc;
        if (!arrayop->useOr || list_length(arrayop->args) != 2 ||
            !integer_equality(get_opcode(arrayop->opno)))
            return NULL;
        left = linitial(arrayop->args);
        right = lsecond(arrayop->args);
        if (!IsA(left, Var) || !integer_array(exprType((Node *) right)))
            return NULL;
        var = (Var *) left;
        if (var->varno != relid || var->varattno != 1 || var->varlevelsup != 0)
            return NULL;
        if (IsA(right, ArrayExpr))
        {
            ArrayExpr *array = (ArrayExpr *) right;
            if (array->multidims) return NULL;
            foreach(lc, array->elements)
            {
                Node *element = lfirst(lc);
                if (!IsA(element, Const) && !IsA(element, Param)) return NULL;
            }
        }
        else if (!IsA(right, Const) && !IsA(right, Param))
            return NULL;
        return right;
    }
    if (!IsA(clause, OpExpr)) return NULL;
    op = (OpExpr *) clause;
    if (list_length(op->args) != 2) return NULL;
    fn = get_opcode(op->opno);
    if (!integer_equality(fn))
        return NULL;
    left = linitial(op->args);
    right = lsecond(op->args);
    if (!IsA(left, Var))
    {
        Expr *tmp = left;
        left = right;
        right = tmp;
    }
    if (!IsA(left, Var) || (!IsA(right, Const) && !IsA(right, Param)))
        return NULL;
    var = (Var *) left;
    if (var->varno != relid || var->varattno != 1 || var->varlevelsup != 0)
        return NULL;
    return right;
}

static void
get_size(PlannerInfo *root, RelOptInfo *rel, Oid tableid)
{
    ListCell *lc;
    (void) root;
    (void) tableid;
    rel->rows = 10000;
    foreach(lc, rel->baserestrictinfo)
    {
        Expr *expr = lookup_expr(lfirst_node(RestrictInfo, lc)->clause, rel->relid);
        if (expr)
            rel->rows = integer_array(exprType((Node *) expr)) ? 100 : 1;
    }
}

static void
get_paths(PlannerInfo *root, RelOptInfo *rel, Oid tableid)
{
    List *pathkeys = NIL;
    (void) tableid;
    /* Full cursors and sorted/deduplicated key batches both emit signed ASC. */
    if (root->query_pathkeys)
        pathkeys = build_expression_pathkey(root,
            (Expr *) makeVar(rel->relid, 1, INT8OID, -1, InvalidOid, 0),
            Int8LessOperator, rel->relids, false);
    add_path(rel, (Path *) create_foreignscan_path(root, rel, NULL, rel->rows,
                5, 5 + rel->rows * 0.02, pathkeys, rel->lateral_relids, NULL,
#if PG_VERSION_NUM >= 170000
                NIL,
#endif
                NIL));
}

static ForeignScan *
get_plan(PlannerInfo *root, RelOptInfo *rel, Oid tableid, ForeignPath *path,
         List *tlist, List *clauses, Plan *outer)
{
    ListCell *lc;
    List *exprs = NIL;
    List *local = NIL;
    List *recheck = NIL;
    Bitmapset *attributes = NULL;
    bool need_value;
    (void) root;
    (void) tableid;
    (void) path;
    clauses = extract_actual_clauses(clauses, false);
    foreach(lc, clauses)
    {
        Expr *expr = exprs ? NULL : lookup_expr(lfirst(lc), rel->relid);
        if (expr)
        {
            exprs = list_make1(expr);
            recheck = list_make1(lfirst(lc));
        }
        else
            local = lappend(local, lfirst(lc));
    }
    pull_varattnos((Node *) tlist, rel->relid, &attributes);
    pull_varattnos((Node *) local, rel->relid, &attributes);
    pull_varattnos((Node *) recheck, rel->relid, &attributes);
    need_value = bms_is_member(2 - FirstLowInvalidHeapAttributeNumber, attributes) ||
                 bms_is_member(0 - FirstLowInvalidHeapAttributeNumber, attributes);
    bms_free(attributes);
    return make_foreignscan(tlist, local, rel->relid, exprs,
                            list_make1(makeInteger(need_value)), NIL, recheck, outer);
}

static void
begin_scan(ForeignScanState *node, int eflags)
{
    ULMDBScanState *s;
    MDB_env *env;
    char *database;
    int rc;
    ForeignScan *plan = (ForeignScan *) node->ss.ps.plan;
    if (eflags & EXEC_FLAG_EXPLAIN_ONLY) return;
    if (node->ss.ps.state->es_plannedstmt->rowMarks != NIL)
        ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
                       errmsg("ulmdb_fdw does not support row-locking clauses")));
    env = get_environment(node->ss.ss_currentRelation, &database);
    s = MemoryContextAllocZero(TopTransactionContext, sizeof(*s));
    s->next = scans;
    scans = s;
    node->fdw_state = s;
    s->need_value = intVal(linitial(plan->fdw_private)) != 0;
    if (plan->fdw_exprs)
    {
        Expr *expr = linitial(plan->fdw_exprs);
        s->key_expr = ExecInitExpr(expr, &node->ss.ps);
        s->key_type = exprType((Node *) expr);
        s->batch = integer_array(s->key_type);
    }
    /* Modifying commands must scan the writer's snapshot before mutating it. */
    if (node->ss.ps.state->es_plannedstmt->commandType != CMD_SELECT)
        get_writer(env);
    if (writer)
        s->txn = writer;
    else
    {
        if (!reader)
            check_mdb(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "begin read");
        s->txn = reader;
    }
#ifdef ULMDB_FDW_SQL_BATCH
    s->sql_batch_enabled = s->txn == reader;
#endif
    rc = open_database(s->txn, database, false, &s->dbi);
    if (rc == MDB_NOTFOUND)
    {
        s->missing_database = true;
        s->done = true;
    }
    else
    {
        check_mdb(rc, "open database");
        if (!s->key_expr)
        {
            check_mdb(mdb_cursor_open(s->txn, s->dbi, &s->cursor), "open cursor");
        }
    }
}

static int
compare_ids(const void *left, const void *right)
{
    int64 a = *(const int64 *) left;
    int64 b = *(const int64 *) right;
    return (a > b) - (a < b);
}

static void
evaluate_batch(ULMDBScanState *s, ForeignScanState *node)
{
    MemoryContext previous;
    Datum datum;
    bool isnull;
    ArrayType *array;
    Datum *elements;
    bool *nulls;
    int count, i;
    Oid element_type;
    int16 length;
    bool byval;
    char alignment;
    s->batch_evaluated = true;
    s->batch_count = s->batch_at = 0;
    previous = MemoryContextSwitchTo(node->ss.ps.state->es_query_cxt);
    datum = ExecEvalExprSwitchContext(s->key_expr,
                                     node->ss.ps.ps_ExprContext, &isnull);
    if (isnull)
    {
        MemoryContextSwitchTo(previous);
        return;
    }
    array = DatumGetArrayTypeP(datum);
    count = ArrayGetNItems(ARR_NDIM(array), ARR_DIMS(array));
    if (count > 65536)
        ereport(ERROR, (errcode(ERRCODE_PROGRAM_LIMIT_EXCEEDED),
                       errmsg("ULMDB primary-key batch exceeds 65536 candidates")));
    element_type = ARR_ELEMTYPE(array);
    if (element_type != INT2OID && element_type != INT4OID && element_type != INT8OID)
        elog(ERROR, "invalid ULMDB primary-key array element type");
    get_typlenbyvalalign(element_type, &length, &byval, &alignment);
    deconstruct_array(array, element_type, length, byval, alignment,
                      &elements, &nulls, &count);
    if (count)
        s->batch_keys = palloc(sizeof(int64) * count);
    for (i = 0; i < count; ++i)
    {
        if (nulls[i]) continue;
        if (element_type == INT2OID)
            s->batch_keys[s->batch_count++] = DatumGetInt16(elements[i]);
        else if (element_type == INT4OID)
            s->batch_keys[s->batch_count++] = DatumGetInt32(elements[i]);
        else
            s->batch_keys[s->batch_count++] = DatumGetInt64(elements[i]);
    }
    if (s->batch_count > 1)
    {
        int unique = 1;
        qsort(s->batch_keys, s->batch_count, sizeof(int64), compare_ids);
        for (i = 1; i < s->batch_count; ++i)
            if (s->batch_keys[i] != s->batch_keys[unique - 1])
                s->batch_keys[unique++] = s->batch_keys[i];
        s->batch_count = unique;
    }
    if (elements) pfree(elements);
    if (nulls) pfree(nulls);
    if ((Pointer) array != DatumGetPointer(datum)) pfree(array);
    MemoryContextSwitchTo(previous);
}

static inline int
fetch_exact_keys(ULMDBScanState *s, ForeignScanState *node, int64 *id, MDB_val *value)
{
    int rc;
    unsigned char bytes[8];
    MDB_val key;
#ifdef ULMDB_FDW_SQL_BATCH
    while (s->sql_batch_enabled)
    {
        if (s->sql_at < s->sql_count)
        {
            *id = s->sql_hits[s->sql_at].id;
            *value = s->sql_hits[s->sql_at++].value;
            return MDB_SUCCESS;
        }
        if (s->batch_at >= s->batch_count) return MDB_NOTFOUND;
        if (!s->sql_hits)
            s->sql_hits = MemoryContextAlloc(node->ss.ps.state->es_query_cxt,
                                            sizeof(*s->sql_hits) * SQL_SCAN_BATCH);
        {
            size_t candidates = Min((size_t) (s->batch_count - s->batch_at),
                                    (size_t) SQL_SCAN_BATCH);
            size_t consumed = 0;
            CHECK_FOR_INTERRUPTS();
            rc = ulmdb_sql_get_batch(s->txn, s->dbi, s->batch_keys + s->batch_at,
                                    candidates, s->sql_hits, SQL_SCAN_BATCH,
                                    &consumed, &s->sql_count);
            if ((rc == MDB_INCOMPATIBLE || rc == MDB_BAD_VALSIZE) &&
                consumed == 0 && s->sql_count == 0)
            {
                s->sql_batch_enabled = false;
                break;
            }
            if (rc != MDB_SUCCESS) return rc;
            if (consumed == 0 || consumed > candidates || s->sql_count > consumed)
                elog(ERROR, "invalid ULMDB bulk lookup response");
            s->batch_at += consumed;
            s->sql_at = 0;
            ++s->sql_refills;
        }
    }
#else
    (void) node;
#endif
    do
    {
        if (s->batch_at >= s->batch_count) return MDB_NOTFOUND;
        *id = s->batch_keys[s->batch_at++];
        encode_id(*id, bytes);
        key.mv_size = 8;
        key.mv_data = bytes;
        rc = mdb_get(s->txn, s->dbi, &key, value);
    } while (rc == MDB_NOTFOUND);
    return rc;
}

static inline int
fetch_cursor(ULMDBScanState *s, ForeignScanState *node, MDB_val *key, MDB_val *value)
{
    int rc;
#ifdef ULMDB_FDW_SQL_BATCH
    if (s->sql_batch_enabled)
    {
        if (s->sql_at == s->sql_count)
        {
            if (!s->sql_rows)
                s->sql_rows = MemoryContextAlloc(node->ss.ps.state->es_query_cxt,
                                                sizeof(*s->sql_rows) * SQL_SCAN_BATCH);
            rc = ulmdb_sql_cursor_batch(s->cursor, s->sql_rows, SQL_SCAN_BATCH,
                                       &s->sql_count, s->started ? MDB_NEXT : MDB_FIRST);
            if ((rc == MDB_INCOMPATIBLE || rc == MDB_BAD_VALSIZE) && s->sql_count == 0)
                s->sql_batch_enabled = false;
            else
            {
                if (rc != MDB_SUCCESS) return rc;
                ++s->sql_refills;
                s->started = true;
                s->sql_at = 0;
            }
        }
        if (s->sql_batch_enabled)
        {
            *key = s->sql_rows[s->sql_at].key;
            *value = s->sql_rows[s->sql_at++].value;
            return MDB_SUCCESS;
        }
    }
#else
    (void) node;
#endif
    rc = mdb_cursor_get(s->cursor, key, value, s->started ? MDB_NEXT : MDB_FIRST);
    s->started = true;
    return rc;
}

static TupleTableSlot *
iterate_scan(ForeignScanState *node)
{
    ULMDBScanState *s = node->fdw_state;
    TupleTableSlot *slot = node->ss.ss_ScanTupleSlot;
    MDB_val key, value;
    unsigned char bytes[8];
    int rc;
    int64 known_id = 0;
    bool id_is_known = false;
    ExecClearTuple(slot);
    if (s->done) return slot;
    if (s->key_expr)
    {
        bool isnull;
        Datum datum;
        int64 id;
        if (s->batch)
        {
            if (!s->batch_evaluated) evaluate_batch(s, node);
            rc = fetch_exact_keys(s, node, &known_id, &value);
            id_is_known = true;
        }
        else
        {
            if (s->started) return slot;
            s->started = true;
            datum = ExecEvalExprSwitchContext(s->key_expr,
                             node->ss.ps.ps_ExprContext, &isnull);
            if (isnull) return slot;
            if (s->key_type == INT2OID) id = DatumGetInt16(datum);
            else if (s->key_type == INT4OID) id = DatumGetInt32(datum);
            else id = DatumGetInt64(datum);
            encode_id(id, bytes);
            key.mv_size = 8;
            key.mv_data = bytes;
            rc = mdb_get(s->txn, s->dbi, &key, &value);
        }
    }
    else
        rc = fetch_cursor(s, node, &key, &value);
    if (rc == MDB_NOTFOUND)
    {
        s->done = true;
        return slot;
    }
    check_mdb(rc, "fetch");
    if (value.mv_size > MaxAllocSize - VARHDRSZ)
        ereport(ERROR, (errcode(ERRCODE_PROGRAM_LIMIT_EXCEEDED),
                       errmsg("ULMDB value exceeds PostgreSQL allocation limit")));
    if (s->need_value)
    {
        Size needed = value.mv_size + VARHDRSZ;
        if (needed > s->payload_capacity)
        {
            Size capacity = Max(needed, Min(MaxAllocSize, Max((Size) 128, s->payload_capacity * 2)));
            if (s->payload)
                s->payload = repalloc(s->payload, capacity);
            else
                s->payload = MemoryContextAlloc(node->ss.ps.state->es_query_cxt, capacity);
            s->payload_capacity = capacity;
        }
        SET_VARSIZE(s->payload, needed);
        memcpy(VARDATA(s->payload), value.mv_data, value.mv_size);
    }
    slot->tts_values[0] = Int64GetDatum(id_is_known ? known_id : decode_id(&key));
    slot->tts_values[1] = PointerGetDatum(s->payload);
    slot->tts_isnull[0] = false;
    slot->tts_isnull[1] = !s->need_value;
    ExecStoreVirtualTuple(slot);
    return slot;
}

static void
rescan(ForeignScanState *node)
{
    ULMDBScanState *s = node->fdw_state;
    s->started = false;
    s->done = s->missing_database || s->txn == NULL;
    s->batch_evaluated = false;
    s->batch_count = s->batch_at = 0;
#ifdef ULMDB_FDW_SQL_BATCH
    s->sql_count = s->sql_at = 0;
#endif
    if (s->batch_keys) pfree(s->batch_keys);
    s->batch_keys = NULL;
}

static void
end_scan(ForeignScanState *node)
{
    ULMDBScanState *s = node->fdw_state;
    ULMDBScanState **link = &scans;
    if (!s) return;
    /* Error paths retain their wrappers for xact cleanup; completed scans do not. */
    while (*link && *link != s)
        link = &(*link)->next;
    if (*link == s)
        *link = s->next;
    close_scan(s);
    if (s->batch_keys) pfree(s->batch_keys);
    if (s->payload) pfree(s->payload);
#ifdef ULMDB_FDW_SQL_BATCH
    if (s->sql_rows) pfree(s->sql_rows);
    if (s->sql_hits) pfree(s->sql_hits);
#endif
    pfree(s);
    node->fdw_state = NULL;
}

static void
explain_scan(ForeignScanState *node, ExplainState *es)
{
    ForeignScan *plan = (ForeignScan *) node->ss.ps.plan;
    const char *access = "full ordered scan";
    if (plan->fdw_exprs)
        access = integer_array(exprType(linitial(plan->fdw_exprs))) ?
                 "primary-key batch" : "primary-key lookup";
    ExplainPropertyText("ULMDB access", access, es);
#ifdef ULMDB_FDW_SQL_BATCH
    ExplainPropertyBool("ULMDB native format checks", true, es);
#else
    ExplainPropertyBool("ULMDB native format checks", false, es);
#endif
#ifdef ULMDB_FDW_SQL_BATCH
    if (node->fdw_state && ((ULMDBScanState *) node->fdw_state)->sql_refills)
        ExplainPropertyInteger("ULMDB batch refills", NULL,
                               ((ULMDBScanState *) node->fdw_state)->sql_refills, es);
#endif
}

static void
add_update_targets(PlannerInfo *root, Index rtindex, RangeTblEntry *rte, Relation rel)
{
    Var *var;
    (void) rte;
    check_schema(rel);
    var = makeVar(rtindex, 1, INT8OID, -1, InvalidOid, 0);
    add_row_identity_var(root, var, rtindex, "ulmdb_id");
}

static List *
plan_modify(PlannerInfo *root, ModifyTable *plan, Index relation, int index)
{
    (void) root;
    (void) relation;
    (void) index;
    if (plan->operation != CMD_INSERT && plan->operation != CMD_UPDATE &&
        plan->operation != CMD_DELETE)
        ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
                       errmsg("ulmdb_fdw supports INSERT, UPDATE and DELETE only")));
    if (plan->onConflictAction != ONCONFLICT_NONE)
        ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
                       errmsg("ulmdb_fdw does not support ON CONFLICT")));
    return NIL;
}

static void
begin_modify(ModifyTableState *mtstate, ResultRelInfo *rinfo,
             List *private, int subplan_index, int eflags)
{
    ModifyState *m;
    MDB_env *env;
    char *database;
    (void) private;
    (void) subplan_index;
    if (eflags & EXEC_FLAG_EXPLAIN_ONLY) return;
    env = get_environment(rinfo->ri_RelationDesc, &database);
    m = palloc0(sizeof(*m));
    rinfo->ri_FdwState = m;
    check_mdb(open_database(get_writer(env), database, true, &m->dbi), "open database");
    if (mtstate->operation == CMD_UPDATE || mtstate->operation == CMD_DELETE)
    {
        m->id_attno = ExecFindJunkAttributeInTlist(outerPlanState(mtstate)->plan->targetlist,
                                                "ulmdb_id");
        if (!AttributeNumberIsValid(m->id_attno))
            elog(ERROR, "ULMDB row identity is missing");
    }
}

static int64
slot_id(TupleTableSlot *slot, AttrNumber attno)
{
    bool isnull;
    Datum datum = slot_getattr(slot, attno, &isnull);
    if (isnull)
        ereport(ERROR, (errcode(ERRCODE_NOT_NULL_VIOLATION),
                       errmsg("ULMDB id must not be null")));
    return DatumGetInt64(datum);
}

static void
put_slot(ModifyState *m, TupleTableSlot *slot, bool insert)
{
    unsigned char bytes[8];
    MDB_val key, value;
    bool isnull;
    Datum datum;
    bytea *payload;
    int rc;
    encode_id(slot_id(slot, 1), bytes);
    key.mv_size = 8;
    key.mv_data = bytes;
    datum = slot_getattr(slot, 2, &isnull);
    if (isnull)
        ereport(ERROR, (errcode(ERRCODE_NOT_NULL_VIOLATION),
                       errmsg("ULMDB value must not be null")));
    payload = DatumGetByteaPP(datum);
    value.mv_size = VARSIZE_ANY_EXHDR(payload);
    value.mv_data = VARDATA_ANY(payload);
    rc = mdb_put(writer, m->dbi, &key, &value, insert ? MDB_NOOVERWRITE : 0);
    if ((Pointer) payload != DatumGetPointer(datum)) pfree(payload);
    if (rc == MDB_KEYEXIST)
        ereport(ERROR, (errcode(ERRCODE_UNIQUE_VIOLATION),
                       errmsg("duplicate ULMDB primary key")));
    check_mdb(rc, "put");
}

static TupleTableSlot *
insert_row(EState *estate, ResultRelInfo *rinfo, TupleTableSlot *slot, TupleTableSlot *plan_slot)
{
    (void) estate;
    (void) plan_slot;
    require_access();
    if (GetCurrentTransactionNestLevel() != 1)
        ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED), errmsg("ULMDB nested writes are unsupported")));
    put_slot(rinfo->ri_FdwState, slot, true);
    return slot;
}

static TupleTableSlot *
update_row(EState *estate, ResultRelInfo *rinfo, TupleTableSlot *slot, TupleTableSlot *plan_slot)
{
    ModifyState *m = rinfo->ri_FdwState;
    (void) estate;
    require_access();
    if (GetCurrentTransactionNestLevel() != 1)
        ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED), errmsg("ULMDB nested writes are unsupported")));
    if (slot_id(plan_slot, m->id_attno) != slot_id(slot, 1))
        ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
                       errmsg("ULMDB primary-key updates are unsupported; delete and insert instead")));
    put_slot(m, slot, false);
    return slot;
}

static TupleTableSlot *
delete_row(EState *estate, ResultRelInfo *rinfo, TupleTableSlot *slot, TupleTableSlot *plan_slot)
{
    ModifyState *m = rinfo->ri_FdwState;
    unsigned char bytes[8];
    MDB_val key, value;
    bytea *payload;
    int64 id;
    int rc;
    (void) estate;
    require_access();
    if (GetCurrentTransactionNestLevel() != 1)
        ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED), errmsg("ULMDB nested writes are unsupported")));
    id = slot_id(plan_slot, m->id_attno);
    encode_id(id, bytes);
    key.mv_size = 8;
    key.mv_data = bytes;
    rc = mdb_get(writer, m->dbi, &key, &value);
    if (rc == MDB_NOTFOUND) return NULL;
    check_mdb(rc, "fetch deleted row");
    if (value.mv_size > MaxAllocSize - VARHDRSZ)
        ereport(ERROR, (errcode(ERRCODE_PROGRAM_LIMIT_EXCEEDED),
                       errmsg("ULMDB value exceeds PostgreSQL allocation limit")));
    payload = palloc(value.mv_size + VARHDRSZ);
    SET_VARSIZE(payload, value.mv_size + VARHDRSZ);
    memcpy(VARDATA(payload), value.mv_data, value.mv_size);
    ExecClearTuple(slot);
    slot->tts_values[0] = Int64GetDatum(id);
    slot->tts_values[1] = PointerGetDatum(payload);
    slot->tts_isnull[0] = slot->tts_isnull[1] = false;
    ExecStoreVirtualTuple(slot);
    rc = mdb_del(writer, m->dbi, &key, NULL);
    if (rc != MDB_NOTFOUND) check_mdb(rc, "delete");
    return slot;
}

static int
updatable(Relation rel)
{
    (void) rel;
    return (1 << CMD_INSERT) | (1 << CMD_UPDATE) | (1 << CMD_DELETE);
}

static void
begin_insert(ModifyTableState *mtstate, ResultRelInfo *rinfo)
{
    (void) mtstate;
    (void) rinfo;
    ereport(ERROR, (errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
                   errmsg("ulmdb_fdw does not support COPY FROM or partition routing")));
}

Datum
ulmdb_fdw_handler(PG_FUNCTION_ARGS)
{
    FdwRoutine *r = makeNode(FdwRoutine);
    r->GetForeignRelSize = get_size;
    r->GetForeignPaths = get_paths;
    r->GetForeignPlan = get_plan;
    r->BeginForeignScan = begin_scan;
    r->IterateForeignScan = iterate_scan;
    r->ReScanForeignScan = rescan;
    r->EndForeignScan = end_scan;
    r->ExplainForeignScan = explain_scan;
    r->AddForeignUpdateTargets = add_update_targets;
    r->PlanForeignModify = plan_modify;
    r->BeginForeignModify = begin_modify;
    r->ExecForeignInsert = insert_row;
    r->ExecForeignUpdate = update_row;
    r->ExecForeignDelete = delete_row;
    r->IsForeignRelUpdatable = updatable;
    r->BeginForeignInsert = begin_insert;
    PG_RETURN_POINTER(r);
}
