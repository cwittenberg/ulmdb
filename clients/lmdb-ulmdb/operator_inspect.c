/* Snapshot statistics and reader maintenance through the public C API. */
#include "operator.h"
#include <errno.h>
#include <string.h>

static int quoted(FILE *out, const char *text) {
    if (fputc('"', out) == EOF) return EIO;
    for (const unsigned char *p = (const unsigned char *)text; *p; ++p) {
        if (*p == '"' || *p == '\\') {
            if (fputc('\\', out) == EOF || fputc(*p, out) == EOF) return EIO;
        } else if (*p < 32 || *p >= 127) {
            if (fprintf(out, "\\u%04x", *p) < 0) return EIO;
        } else if (fputc(*p, out) == EOF) return EIO;
    }
    return fputc('"', out) == EOF ? EIO : 0;
}

struct reader_output { int json, first, error; };
static int reader_line(const char *line, void *context) {
    struct reader_output *out = context;
    if (out->json) {
        if (!out->first && putchar(',') == EOF) out->error = EIO;
        out->first = 0;
        if (!out->error) out->error = quoted(stdout, line);
    } else if (fputs(line, stdout) == EOF) out->error = EIO;
    return out->error ? 1 : 0;
}

int tool_readers(MDB_env *env, int clear, int json) {
    int dead = 0, rc;
    if (clear && (rc = mdb_reader_check(env, &dead))) return rc;
    struct reader_output out = {json, 1, 0};
    if (json && printf("{\"schema\":1,\"cleanup_requested\":%s,\"cleared\":%d,\"lines\":[",
                       clear ? "true" : "false", dead) < 0) return EIO;
    rc = mdb_reader_list(env, reader_line, &out);
    if (out.error) return out.error;
    if (rc) return rc;
    if (json) return puts("]}") == EOF ? EIO : 0;
    if (clear && printf("cleared=%d\n", dead) < 0) return EIO;
    return 0;
}

int tool_inspect(MDB_env *env, int verify, int json) {
    MDB_txn *txn = NULL;
    struct tool_database *dbs = NULL;
    size_t count = 0;
    MDB_envinfo info;
    int rc = mdb_env_info(env, &info);
    if (rc) return rc;
    if ((rc = mdb_txn_begin(env, NULL, MDB_RDONLY, &txn))) return rc;
    if ((rc = tool_databases(txn, &dbs, &count))) goto done;
    if (json && printf("{\"schema\":1,\"transaction\":%zu,\"map_size\":%zu,"
                       "\"max_readers\":%u,\"used_reader_slots\":%u,\"databases\":[",
                       mdb_txn_id(txn), info.me_mapsize, info.me_maxreaders,
                       info.me_numreaders) < 0) { rc = EIO; goto done; }
    for (size_t i = 0; i < count; ++i) {
        MDB_stat stat;
        if ((rc = mdb_stat(txn, dbs[i].dbi, &stat))) goto done;
        if (verify) {
            MDB_cursor *cursor;
            MDB_val key, value;
            if ((rc = mdb_cursor_open(txn, dbs[i].dbi, &cursor))) goto done;
            while ((rc = mdb_cursor_get(cursor, &key, &value, MDB_NEXT)) == 0) {}
            mdb_cursor_close(cursor);
            if (rc != MDB_NOTFOUND) goto done;
            rc = 0;
        }
        if (fputs(json ? (i ? ",{\"name_hex\":\"" : "{\"name_hex\":\"") : "database=", stdout) == EOF) {
            rc = EIO; goto done;
        }
        if ((rc = tool_hex(stdout, dbs[i].name, dbs[i].length))) goto done;
        int written = json
            ? printf("\",\"flags\":%u,\"page_size\":%u,\"depth\":%u,\"branch\":%zu,"
                     "\"leaf\":%zu,\"overflow\":%zu,\"entries\":%zu}",
                     dbs[i].flags, stat.ms_psize, stat.ms_depth, stat.ms_branch_pages,
                     stat.ms_leaf_pages, stat.ms_overflow_pages, stat.ms_entries)
            : printf(" flags=%u page_size=%u depth=%u branch=%zu leaf=%zu overflow=%zu entries=%zu\n",
                     dbs[i].flags, stat.ms_psize, stat.ms_depth, stat.ms_branch_pages,
                     stat.ms_leaf_pages, stat.ms_overflow_pages, stat.ms_entries);
        if (written < 0) { rc = EIO; goto done; }
    }
    if (json) {
        if (printf("],\"structural_check\":%s,\"custom_order_certified\":false}\n",
                   verify ? "true" : "false") < 0) rc = EIO;
    } else if (verify && printf("check=ok databases=%zu structural=1 custom-order-not-certified=1\n", count) < 0) rc = EIO;
done:
    tool_free_databases(dbs, count);
    mdb_txn_abort(txn);
    return rc;
}
