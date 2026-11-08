/* One real C application is loaded unchanged against both adapter artifacts. */
#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include <lmdb.h>
#include <dlfcn.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>

#define VALUE_BYTES 64
struct phase {
    const char *name, *unit;
    uint64_t operations, transactions, elapsed, count, *samples;
};
static void require(int condition, const char *operation) {
    if (!condition) { fprintf(stderr, "oracle failed: %s\n", operation); exit(1); }
}
static void ok(int code, const char *operation) {
    if (code) { fprintf(stderr, "%s: %s (%d)\n", operation, mdb_strerror(code), code); exit(1); }
}
static uint64_t now(void) {
    struct timespec value;
    require(!clock_gettime(CLOCK_MONOTONIC_RAW, &value), "clock");
    return (uint64_t)value.tv_sec * UINT64_C(1000000000) + (uint64_t)value.tv_nsec;
}
static unsigned number(const char *text, unsigned maximum) {
    char *end;
    unsigned long value = strtoul(text, &end, 10);
    require(*text && !*end && value && value <= maximum, "bounded positive argument");
    return (unsigned)value;
}
static void encoded(uint64_t value, unsigned char bytes[8]) {
    for (unsigned i = 0; i < 8; ++i) bytes[i] = (unsigned char)(value >> ((7-i)*8));
}
static void value_for(unsigned key, uint64_t version, unsigned char bytes[VALUE_BYTES]) {
    encoded(key, bytes); encoded(version, bytes + 8);
    for (unsigned i = 16; i < VALUE_BYTES; ++i)
        bytes[i] = (unsigned char)((uint64_t)key * 17 + version * 29 + i * 7);
}
static void put(MDB_txn *txn, MDB_dbi db, unsigned key_id, uint64_t version) {
    unsigned char key_bytes[8], value_bytes[VALUE_BYTES];
    encoded(key_id, key_bytes); value_for(key_id, version, value_bytes);
    MDB_val key = {sizeof key_bytes, key_bytes}, value = {sizeof value_bytes, value_bytes};
    ok(mdb_put(txn, db, &key, &value, 0), "put");
}
static MDB_val get(MDB_txn *txn, MDB_dbi db, unsigned key_id, uint64_t version) {
    unsigned char key_bytes[8], expected[VALUE_BYTES];
    encoded(key_id, key_bytes); value_for(key_id, version, expected);
    MDB_val key = {sizeof key_bytes, key_bytes}, value;
    ok(mdb_get(txn, db, &key, &value), "get");
    require(value.mv_size == sizeof expected && !memcmp(value.mv_data, expected, sizeof expected), "get bytes");
    return value;
}
static uint64_t hash_bytes(uint64_t hash, const void *data, size_t size) {
    const unsigned char *bytes = data;
    for (size_t i = 0; i < size; ++i) hash = (hash ^ bytes[i]) * UINT64_C(1099511628211);
    return hash;
}
static uint64_t image(MDB_txn *txn, MDB_dbi db, unsigned keys, const uint64_t *versions) {
    MDB_cursor *cursor;
    ok(mdb_cursor_open(txn, db, &cursor), "cursor_open");
    MDB_val key, value;
    uint64_t hash = UINT64_C(14695981039346656037);
    int code = mdb_cursor_get(cursor, &key, &value, MDB_FIRST);
    unsigned count = 0;
    while (!code) {
        unsigned char expected_key[8], expected_value[VALUE_BYTES];
        require(count < keys, "extra record");
        encoded(count, expected_key); value_for(count, versions ? versions[count] : 0, expected_value);
        require(key.mv_size == sizeof expected_key && !memcmp(key.mv_data, expected_key, sizeof expected_key), "cursor key order/coverage");
        require(value.mv_size == sizeof expected_value && !memcmp(value.mv_data, expected_value, sizeof expected_value), "cursor value bytes");
        hash = hash_bytes(hash_bytes(hash, key.mv_data, key.mv_size), value.mv_data, value.mv_size);
        ++count;
        code = mdb_cursor_get(cursor, &key, &value, MDB_NEXT);
    }
    require(code == MDB_NOTFOUND && count == keys, "complete cursor image");
    mdb_cursor_close(cursor);
    return hash;
}
static MDB_env *open_environment(const char *path) {
    MDB_env *env;
    ok(mdb_env_create(&env), "env_create");
    ok(mdb_env_set_mapsize(env, 64u * 1024u * 1024u), "set_mapsize");
    ok(mdb_env_set_maxdbs(env, 8), "set_maxdbs");
    ok(mdb_env_set_maxreaders(env, 16), "set_maxreaders");
    ok(mdb_env_open(env, path, MDB_NOTLS, 0600), "env_open");
    return env;
}
static struct phase phase(const char *name, const char *unit, uint64_t operations, uint64_t transactions, unsigned samples) {
    struct phase result = {name, unit, operations, transactions, 0, samples, calloc(samples, sizeof(uint64_t))};
    require(result.samples != NULL, "sample allocation");
    return result;
}
static uint64_t update(MDB_env *env, MDB_dbi db, unsigned keys, unsigned transactions, unsigned batch,
                       uint64_t *versions, uint64_t *step, MDB_val pinned, struct phase *metric) {
    uint64_t start = now();
    for (unsigned t = 0; t < transactions; ++t) {
        uint64_t before = now();
        MDB_txn *writer;
        ok(mdb_txn_begin(env, NULL, 0, &writer), "update_begin");
        for (unsigned i = 0; i < batch; ++i) {
            unsigned key = (unsigned)(((*step)++ * 37 + 17) % keys);
            put(writer, db, key, ++versions[key]);
        }
        ok(mdb_txn_commit(writer), "update_commit");
        metric->samples[t] = now() - before;
        unsigned char old[VALUE_BYTES]; value_for(0, 0, old);
        require(pinned.mv_size == sizeof old && !memcmp(pinned.mv_data, old, sizeof old), "old borrowed pointer survives publication");
    }
    metric->elapsed = now() - start;
    MDB_txn *fresh;
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &fresh), "fresh_begin");
    uint64_t checksum = image(fresh, db, keys, versions);
    mdb_txn_abort(fresh);
    return checksum;
}
static void json_string(const char *text) {
    putchar('"');
    for (const unsigned char *p = (const unsigned char *)text; *p; ++p) {
        if (*p == '"' || *p == '\\') { putchar('\\'); putchar(*p); }
        else if (*p < 32 || *p >= 127) printf("\\u%04x", *p);
        else putchar(*p);
    }
    putchar('"');
}
int main(int argc, char **argv) {
    require(argc == 7, "usage: journey NEW_DIR KEYS BATCH_READS SINGLE_READS WRITE_TXNS REOPENS");
    unsigned keys = number(argv[2], 65536), batch_reads = number(argv[3], 1000000);
    unsigned single_reads = number(argv[4], 4096), writes = number(argv[5], 4096), reopens = number(argv[6], 128);
    uint64_t *versions = calloc(keys, sizeof(uint64_t)); require(versions != NULL, "version allocation");
    require(!mkdir(argv[1], 0700), "fresh database directory");
    Dl_info provider;
    require(dladdr((void *)mdb_env_open, &provider) && provider.dli_fname, "actual symbol provider");
    struct phase metrics[] = {
        phase("create_open", "environment", 1, 0, 1),
        phase("initial_load", "write_transaction", keys, 1, 1),
        phase("batched_get", "get_with_value_oracle", batch_reads, 1, batch_reads),
        phase("single_get_transaction", "read_transaction_with_get", single_reads, single_reads, single_reads),
        phase("update_batch1", "write_transaction_1_record", writes, writes, writes),
        phase("update_batch16", "write_transaction_16_records", (uint64_t)writes * 16, writes, writes),
        phase("close_reopen", "environment_close_and_open", reopens, 0, reopens)
    };
    uint64_t start = now();
    MDB_env *env = open_environment(argv[1]);
    metrics[0].elapsed = metrics[0].samples[0] = now() - start;
    start = now();
    MDB_txn *writer, *reader;
    MDB_dbi db;
    ok(mdb_txn_begin(env, NULL, 0, &writer), "load_begin");
    ok(mdb_dbi_open(writer, "records", MDB_CREATE, &db), "dbi_open");
    for (unsigned i = 0; i < keys; ++i) put(writer, db, i, 0);
    ok(mdb_txn_commit(writer), "load_commit");
    metrics[1].elapsed = metrics[1].samples[0] = now() - start;
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "loaded_read");
    uint64_t loaded = image(reader, db, keys, NULL);
    mdb_txn_abort(reader);
    start = now();
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "batch_read_begin");
    for (unsigned i = 0; i < batch_reads; ++i) {
        uint64_t before = now();
        (void)get(reader, db, (i * 37u + 17u) % keys, 0);
        metrics[2].samples[i] = now() - before;
    }
    mdb_txn_abort(reader);
    metrics[2].elapsed = now() - start;
    start = now();
    for (unsigned i = 0; i < single_reads; ++i) {
        uint64_t before = now();
        ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "single_read_begin");
        (void)get(reader, db, (i * 37u + 17u) % keys, 0);
        mdb_txn_abort(reader);
        metrics[3].samples[i] = now() - before;
    }
    metrics[3].elapsed = now() - start;
    MDB_txn *old;
    ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &old), "old_snapshot_begin");
    MDB_val pinned = get(old, db, 0, 0);
    uint64_t step = 0;
    uint64_t batch1 = update(env, db, keys, writes, 1, versions, &step, pinned, &metrics[4]);
    require(image(old, db, keys, NULL) == loaded, "old snapshot after batch1");
    uint64_t final = update(env, db, keys, writes, 16, versions, &step, pinned, &metrics[5]);
    require(image(old, db, keys, NULL) == loaded, "old snapshot after batch16");
    mdb_txn_abort(old);
    for (unsigned i = 0; i < reopens; ++i) {
        uint64_t before = now();
        mdb_env_close(env);
        env = open_environment(argv[1]);
        metrics[6].samples[i] = now() - before;
        metrics[6].elapsed += metrics[6].samples[i];
        ok(mdb_txn_begin(env, NULL, MDB_RDONLY, &reader), "reopened_read");
        ok(mdb_dbi_open(reader, "records", 0, &db), "reopened_dbi");
        require(image(reader, db, keys, versions) == final, "reopened complete image");
        mdb_txn_abort(reader);
    }
    mdb_env_close(env);
    printf("{\"schema\":1,\"passed\":true,\"provider\":"); json_string(provider.dli_fname);
    printf(",\"keys\":%u,\"value_bytes\":%u,\"checksums\":{\"loaded\":\"%016" PRIx64 "\",\"after_batch1\":\"%016" PRIx64 "\",\"final\":\"%016" PRIx64 "\"},\"metrics\":[", keys, VALUE_BYTES, loaded, batch1, final);
    for (unsigned i = 0; i < sizeof metrics / sizeof *metrics; ++i) {
        struct phase *metric = metrics + i;
        if (i) putchar(',');
        printf("{\"phase\":"); json_string(metric->name);
        printf(",\"sample_unit\":"); json_string(metric->unit);
        printf(",\"operations\":%" PRIu64 ",\"transactions\":%" PRIu64 ",\"elapsed_ns\":%" PRIu64 ",\"latency_ns\":[", metric->operations, metric->transactions, metric->elapsed);
        for (uint64_t j = 0; j < metric->count; ++j) printf("%s%" PRIu64, j ? "," : "", metric->samples[j]);
        printf("]}"); free(metric->samples);
    }
    puts("]}"); free(versions);
    return 0;
}
