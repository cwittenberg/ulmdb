/* NEW actual public MAIN reopen oracle. The prior similarly named probe did
 * not reopen MAIN and is retained solely as corrected historical evidence. */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

static unsigned key_calls, dup_calls, failures;
static void ok(int rc, const char *label) {
    if (rc) { fprintf(stderr, "%s=%d\n", label, rc); exit(2); }
}
static void require(int yes, const char *label) {
    if (!yes) { fprintf(stderr, "MAIN reopen mismatch: %s\n", label); failures++; }
}
static MDB_val val(const char *text) {
    MDB_val result = {strlen(text), (void *)text}; return result;
}
static int fold(const MDB_val *a, const MDB_val *b) {
    size_t n = a->mv_size < b->mv_size ? a->mv_size : b->mv_size;
    for (size_t i = 0; i < n; ++i) {
        unsigned char x = ((const unsigned char *)a->mv_data)[i];
        unsigned char y = ((const unsigned char *)b->mv_data)[i];
        if (x >= 'A' && x <= 'Z') x += 'a' - 'A';
        if (y >= 'A' && y <= 'Z') y += 'a' - 'A';
        if (x != y) return (x > y) - (x < y);
    }
    return (a->mv_size > b->mv_size) - (a->mv_size < b->mv_size);
}
static int key_fold(const MDB_val *a, const MDB_val *b) { key_calls++; return fold(a, b); }
static int dup_fold(const MDB_val *a, const MDB_val *b) { dup_calls++; return fold(a, b); }
static void comparisons(MDB_txn *txn, MDB_dbi main, int duplicates,
                        int custom, const char *phase) {
    MDB_val a = val("X"), b = val("x");
    key_calls = dup_calls = 0;
    int rc = mdb_cmp(txn, main, &a, &b);
    printf("%s.key.equal=%d key_calls=%u dup_calls=%u\n", phase, rc == 0, key_calls, dup_calls);
    require((rc == 0) == custom && key_calls == (unsigned)custom && dup_calls == 0, "key callback/reset");
    if (duplicates) {
        key_calls = dup_calls = 0;
        rc = mdb_dcmp(txn, main, &a, &b);
        printf("%s.dup.equal=%d key_calls=%u dup_calls=%u\n", phase, rc == 0, key_calls, dup_calls);
        require((rc == 0) == custom && key_calls == 0 && dup_calls == (unsigned)custom, "duplicate callback/reset");
    }
}
static void configure(MDB_txn *txn, MDB_dbi main, int duplicates) {
    ok(mdb_set_compare(txn, main, key_fold), "key registration before data");
    if (duplicates) ok(mdb_set_dupsort(txn, main, dup_fold), "duplicate registration before data");
}
static void data_oracle(MDB_txn *txn, MDB_dbi main, int duplicates) {
    MDB_stat stats; ok(mdb_stat(txn, main, &stats), "stat data");
    printf("entries=%zu\n", stats.ms_entries);
    require(stats.ms_entries == (size_t)(duplicates ? 2 : 1), "real entry count");
    MDB_val key = val(duplicates ? "M" : "X"), data;
    ok(mdb_get(txn, main, &key, &data), "real get");
    printf("get.value=%.*s\n", (int)data.mv_size, (char *)data.mv_data);
    const char *expected = duplicates ? "x" : "two";
    require(data.mv_size == strlen(expected) && !memcmp(data.mv_data, expected, data.mv_size), "real value");
    if (duplicates) {
        MDB_cursor *cursor; ok(mdb_cursor_open(txn, main, &cursor), "duplicate cursor");
        key = val("M"); ok(mdb_cursor_get(cursor, &key, &data, MDB_SET), "first duplicate");
        require(data.mv_size == 1 && !memcmp(data.mv_data, "x", 1), "fold first duplicate");
        ok(mdb_cursor_get(cursor, &key, &data, MDB_NEXT_DUP), "second duplicate");
        printf("next_duplicate.value=%.*s\n", (int)data.mv_size, (char *)data.mv_data);
        require(data.mv_size == 1 && !memcmp(data.mv_data, "Y", 1), "fold second duplicate");
        int rc = mdb_cursor_get(cursor, &key, &data, MDB_NEXT_DUP);
        printf("duplicate_end=%d\n", rc); require(rc == MDB_NOTFOUND, "duplicate end");
        mdb_cursor_close(cursor);
    }
}
static void scenario(const char *base, int duplicates, int notls) {
    char path[4096]; int n = snprintf(path, sizeof path, "%s/%d-%d", base, duplicates, notls);
    require(n > 0 && (size_t)n < sizeof path, "path");
    if (mkdir(path, 0700)) { perror("scenario directory"); exit(2); }
    printf("scenario=MAIN-public-reopen-%s-%s\n", duplicates ? "duplicate" : "key", notls ? "notls" : "tls");
    MDB_env *env; MDB_txn *txn; MDB_dbi main, reopened;
    ok(mdb_env_create(&env), "create"); ok(mdb_env_set_mapsize(env, 16u * 1024u * 1024u), "mapsize");
    ok(mdb_env_open(env, path, notls ? MDB_NOTLS : 0, 0600), "environment open");
    ok(mdb_txn_begin(env, NULL, 0, &txn), "writer begin");
    /* First public open initializes the public MAIN flags/defaults. */
    ok(mdb_dbi_open(txn, NULL, duplicates ? MDB_DUPSORT : 0, &main), "initial public MAIN open");
    configure(txn, main, duplicates);
    comparisons(txn, main, duplicates, 1, "before_public_reopen");
    /* The second actual public open MUST reset both custom comparators. */
    int rc = mdb_dbi_open(txn, NULL, 0, &reopened);
    printf("public_reopen.rc=%d same_dbi=%d\n", rc, rc == 0 && reopened == main);
    ok(rc, "second public MAIN open"); require(reopened == main, "same public MAIN DBI");
    comparisons(txn, reopened, duplicates, 0, "after_public_reopen");
    unsigned flags; ok(mdb_dbi_flags(txn, main, &flags), "flags preserved");
    printf("flags=%u\n", flags); require(flags == (unsigned)(duplicates ? MDB_DUPSORT : 0), "flags survive default reset");
    /* Re-register before the first data access, as required by LMDB. */
    configure(txn, reopened, duplicates);
    comparisons(txn, reopened, duplicates, 1, "after_reregister");
    MDB_val key = val(duplicates ? "m" : "X"), data = val(duplicates ? "Y" : "one");
    ok(mdb_put(txn, main, &key, &data, 0), "first real put");
    if (!duplicates) key = val("x");
    data = val(duplicates ? "x" : "two");
    ok(mdb_put(txn, main, &key, &data, 0), "second real put");
    data_oracle(txn, main, duplicates);
    ok(mdb_txn_commit(txn), "writer commit");
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &txn), "fresh reader");
    comparisons(txn, main, duplicates, 1, "after_commit");
    data_oracle(txn, main, duplicates);
    mdb_txn_abort(txn); mdb_env_close(env);
}
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 2;
    for (int duplicates = 0; duplicates != 2; ++duplicates)
        for (int notls = 0; notls != 2; ++notls) scenario(argv[1], duplicates, notls);
    printf("MAIN-reopen-complete failures=%u\n", failures); return failures ? 1 : 0;
}
