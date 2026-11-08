/* Compare observable transaction and cursor behavior against real LMDB. */
#include "parity_workload_data.h"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void parity_require(int condition, const char *operation) {
    if (!condition) { fprintf(stderr, "workload invariant: %s\n", operation); exit(1); }
}
void parity_check(int result, const char *operation) {
    if (result) {
        fprintf(stderr, "workload %s: %d %s\n", operation, result, mdb_strerror(result));
        exit(1);
    }
}
uint32_t parity_random_next(struct parity_random *random) {
    uint64_t value = random->state;
    value ^= value >> 12; value ^= value << 25; value ^= value >> 27;
    random->state = value;
    return (uint32_t)((value * UINT64_C(2685821657736338717)) >> 32);
}
static void hex(MDB_val value) {
    const unsigned char *bytes = value.mv_data;
    printf("%zu:", value.mv_size);
    for (size_t i = 0; i < value.mv_size; ++i) printf("%02x", bytes[i]);
}
static MDB_val key_for(unsigned id, unsigned char bytes[8]) {
    bytes[0] = (unsigned char)(id / 16);
    bytes[1] = (unsigned char)(id * 37);
    bytes[2] = (unsigned char)id;
    bytes[3] = 0; bytes[4] = 255; bytes[5] = (unsigned char)(id ^ 0x80);
    MDB_val result = {1 + id % 6, bytes};
    return result;
}
static MDB_val value_for(unsigned id, int duplicates, unsigned char bytes[16]) {
    for (unsigned i = 0; i < 16; ++i)
        bytes[i] = (unsigned char)((id * (i + 7) + i * 29) ^ (i % 3 ? 0x80 : 0));
    bytes[0] = (unsigned char)id;
    MDB_val result = {duplicates ? 1 + id % 11 : id % 13, bytes};
    return result;
}
static void result_value(int result, MDB_val value) {
    printf(" result=%d", result);
    if (!result) { putchar(' '); hex(value); }
}
static void current(MDB_cursor *cursor) {
    MDB_val key = {0, NULL}, value = {0, NULL};
    int result = mdb_cursor_get(cursor, &key, &value, MDB_GET_CURRENT);
    printf(" current=%d", result);
    if (!result) { putchar(' '); hex(key); putchar('='); hex(value); }
}
static int probe(MDB_cursor *cursor, MDB_cursor_op operation) {
    MDB_val key = {0, NULL}, value = {0, NULL};
    int result = mdb_cursor_get(cursor, &key, &value, operation);
    printf(" move=%u:%d", (unsigned)operation, result);
    if (!result) current(cursor);
    return result;
}
static int stays_in_duplicate_slot(MDB_txn *txn, MDB_dbi db, MDB_val key,
                                  MDB_val old, MDB_val replacement) {
    MDB_cursor *check;
    parity_check(mdb_cursor_open(txn, db, &check), "replacement cursor_open");
    MDB_val seek_key = key, seek_value = old;
    parity_check(mdb_cursor_get(check, &seek_key, &seek_value, MDB_GET_BOTH), "replacement current");
    MDB_val neighbor_key = {0, NULL}, neighbor = {0, NULL};
    int result = mdb_cursor_get(check, &neighbor_key, &neighbor, MDB_PREV_DUP);
    int valid = result == MDB_NOTFOUND || (!result && mdb_dcmp(txn, db, &neighbor, &replacement) < 0);
    parity_require(!result || result == MDB_NOTFOUND, "replacement previous duplicate");
    seek_key = key; seek_value = old;
    parity_check(mdb_cursor_get(check, &seek_key, &seek_value, MDB_GET_BOTH), "replacement restore");
    result = mdb_cursor_get(check, &neighbor_key, &neighbor, MDB_NEXT_DUP);
    valid = valid && (result == MDB_NOTFOUND || (!result && mdb_dcmp(txn, db, &replacement, &neighbor) < 0));
    parity_require(!result || result == MDB_NOTFOUND, "replacement next duplicate");
    mdb_cursor_close(check);
    return valid;
}
static void cursor_operation(MDB_txn *txn, MDB_dbi db, MDB_val key,
                             MDB_val value, int duplicates, unsigned action,
                             uint32_t choice) {
    MDB_cursor *cursor;
    parity_check(mdb_cursor_open(txn, db, &cursor), "cursor_open");
    MDB_cursor_op seek = action == 7 && duplicates ?
        (choice & 1 ? MDB_GET_BOTH : MDB_GET_BOTH_RANGE) :
        action == 13 ? MDB_SET_KEY : MDB_SET_RANGE;
    int result = mdb_cursor_get(cursor, &key, &value, seek);
    printf(" seek=%u:%d", (unsigned)seek, result);
    if (!result) {
        current(cursor);
        if (action == 8) {
            unsigned flags = duplicates && choice & 1 ? MDB_NODUPDATA : 0;
            result = mdb_cursor_del(cursor, flags);
            printf(" delete=%u:%d", flags, result);
            if (!result) {
                probe(cursor, MDB_GET_CURRENT);
                probe(cursor, MDB_NEXT);
                probe(cursor, MDB_PREV);
            }
        } else if (action == 10) {
            /* Own write inputs: a cursor update may move a writable LMDB page. */
            unsigned char replacement[16], owned_key[8], old_bytes[16];
            parity_require(key.mv_size <= sizeof owned_key, "generated key size");
            memcpy(owned_key, key.mv_data, key.mv_size);
            key.mv_data = owned_key;
            MDB_val next = value_for((choice >> 5) % 32, duplicates, replacement);
            if (duplicates) {
                parity_require(value.mv_size <= sizeof old_bytes, "generated duplicate size");
                memcpy(old_bytes, value.mv_data, value.mv_size);
                MDB_val old = {value.mv_size, old_bytes};
                /* CURRENT is intended for same-size, same-sort-slot updates. */
                next.mv_size = old.mv_size;
                if (!stays_in_duplicate_slot(txn, db, key, old, next)) next = old;
            }
            result = mdb_cursor_put(cursor, &key, &next, MDB_CURRENT);
            printf(" replace=%d", result);
            if (!result) current(cursor);
        } else if (action == 7 && duplicates) {
            size_t count = 0;
            result = mdb_cursor_count(cursor, &count);
            printf(" count=%d:%zu", result, count);
            probe(cursor, MDB_FIRST_DUP);
            probe(cursor, MDB_NEXT_DUP);
            probe(cursor, MDB_LAST_DUP);
            probe(cursor, MDB_PREV_DUP);
        } else {
            probe(cursor, MDB_NEXT);
            probe(cursor, MDB_PREV);
            if (duplicates) {
                probe(cursor, MDB_NEXT_NODUP);
                probe(cursor, MDB_PREV_NODUP);
            }
        }
    }
    mdb_cursor_close(cursor);
}
void parity_operation(MDB_txn *txn, MDB_dbi normal, MDB_dbi duplicate,
                       struct parity_random *random) {
    unsigned choice = parity_random_next(random);
    int duplicates = (choice >> 4) & 1;
    unsigned action = choice % 16;
    unsigned key_id = parity_random_next(random) % 64;
    unsigned value_id = parity_random_next(random) % 32;
    MDB_dbi db = duplicates ? duplicate : normal;
    unsigned char key_bytes[8] = {0}, value_bytes[16];
    MDB_val key = key_for(key_id, key_bytes), value = value_for(value_id, duplicates, value_bytes);
    printf("operation=%" PRIu64 " db=%d action=%u key=", random->step++, duplicates, action);
    hex(key); fputs(" value=", stdout); hex(value);
    int result;
    if (action <= 2 || action == 14) {
        unsigned flags = action == 1 ? MDB_NOOVERWRITE :
            action == 2 && duplicates ? MDB_NODUPDATA :
            action == 14 ? (duplicates ? MDB_APPENDDUP : MDB_APPEND) : 0;
        result = mdb_put(txn, db, &key, &value, flags);
        printf(" put=%u:%d", flags, result);
        if (flags == MDB_NOOVERWRITE && result == MDB_KEYEXIST) {
            fputs(" existing=", stdout); hex(value);
        }
    } else if (action == 3 || action == 4) {
        result = mdb_del(txn, db, &key, action == 4 ? &value : NULL);
        printf(" delete=%d", result);
    } else if (action == 5 || action == 15) {
        if (action == 15) { key_bytes[0] = 255; key.mv_size = 7; }
        MDB_val found = {0, NULL};
        result = mdb_get(txn, db, &key, &found);
        result_value(result, found);
    } else if (action == 6 || action == 7 || action == 8 || action == 10 || action == 13) {
        cursor_operation(txn, db, key, value, duplicates, action, choice);
    } else if (action == 9) {
        MDB_cursor *cursor;
        parity_check(mdb_cursor_open(txn, db, &cursor), "put_cursor_open");
        result = mdb_cursor_put(cursor, &key, &value, duplicates && choice & 2 ? MDB_NODUPDATA : 0);
        printf(" cursor-put=%d", result);
        if (!result) current(cursor);
        mdb_cursor_close(cursor);
    } else if (action == 11 && !duplicates) {
        MDB_val reserved = {value.mv_size, NULL};
        result = mdb_put(txn, db, &key, &reserved, MDB_RESERVE);
        printf(" reserve=%d", result);
        if (!result) {
            if (value.mv_size) memcpy(reserved.mv_data, value.mv_data, value.mv_size);
            MDB_val found = {0, NULL};
            parity_check(mdb_get(txn, db, &key, &found), "read reserve");
            parity_require(found.mv_size == value.mv_size &&
                (!value.mv_size || !memcmp(found.mv_data, value.mv_data, value.mv_size)), "reserve bytes readable");
            for (size_t i = 0; i < reserved.mv_size; ++i)
                ((unsigned char *)reserved.mv_data)[i] ^= 0xa5;
            parity_check(mdb_get(txn, db, &key, &found), "reread reserve");
            result_value(0, found);
        }
    } else {
        unsigned char other_bytes[8];
        MDB_val other = key_for((key_id + 17) % 64, other_bytes);
        int compared = mdb_cmp(txn, db, &key, &other);
        printf(" compare=%d", (compared > 0) - (compared < 0));
        if (duplicates) {
            unsigned char next_bytes[16];
            MDB_val next = value_for((value_id + 7) % 32, 1, next_bytes);
            compared = mdb_dcmp(txn, db, &value, &next);
            printf(" dupcompare=%d", (compared > 0) - (compared < 0));
        }
    }
    putchar('\n');
}
static uint64_t hash_byte(uint64_t hash, unsigned char byte) {
    return (hash ^ byte) * UINT64_C(1099511628211);
}
static uint64_t hash_value(uint64_t hash, MDB_val value) {
    uint64_t size = value.mv_size;
    for (unsigned i = 0; i < 8; ++i) hash = hash_byte(hash, (unsigned char)(size >> (8 * i)));
    const unsigned char *bytes = value.mv_data;
    for (size_t i = 0; i < value.mv_size; ++i) hash = hash_byte(hash, bytes[i]);
    return hash;
}
static int byte_order(MDB_val left, MDB_val right) {
    size_t common = left.mv_size < right.mv_size ? left.mv_size : right.mv_size;
    int compared = common ? memcmp(left.mv_data, right.mv_data, common) : 0;
    if (compared) return (compared > 0) - (compared < 0);
    return (left.mv_size > right.mv_size) - (left.mv_size < right.mv_size);
}
static uint64_t scan(MDB_txn *txn, MDB_dbi db, int reverse, uint64_t *count) {
    MDB_cursor *cursor;
    parity_check(mdb_cursor_open(txn, db, &cursor), "scan cursor_open");
    MDB_val key = {0, NULL}, value = {0, NULL};
    uint64_t hash = UINT64_C(14695981039346656037);
    unsigned char previous_key_bytes[8], previous_value_bytes[16];
    MDB_val previous_key = {0, previous_key_bytes}, previous_value = {0, previous_value_bytes};
    *count = 0;
    int result = mdb_cursor_get(cursor, &key, &value, reverse ? MDB_LAST : MDB_FIRST);
    while (!result) {
        parity_require(key.mv_size <= sizeof previous_key_bytes && value.mv_size <= sizeof previous_value_bytes,
                       "generated record size bound");
        if (*count) {
            int order = byte_order(previous_key, key);
            if (!order) order = byte_order(previous_value, value);
            parity_require(reverse ? order > 0 : order < 0, "cursor traversal is strictly ordered");
        }
        memcpy(previous_key_bytes, key.mv_data, key.mv_size);
        if (value.mv_size) memcpy(previous_value_bytes, value.mv_data, value.mv_size);
        previous_key.mv_size = key.mv_size; previous_value.mv_size = value.mv_size;
        hash = hash_value(hash_value(hash, key), value);
        ++*count;
        result = mdb_cursor_get(cursor, &key, &value, reverse ? MDB_PREV : MDB_NEXT);
    }
    parity_require(result == MDB_NOTFOUND, "scan ends at MDB_NOTFOUND");
    mdb_cursor_close(cursor);
    return hash;
}
struct parity_image parity_image(MDB_txn *txn, MDB_dbi db) {
    struct parity_image image;
    uint64_t reverse_count;
    image.forward = scan(txn, db, 0, &image.entries);
    image.reverse = scan(txn, db, 1, &reverse_count);
    parity_require(image.entries == reverse_count, "forward/reverse counts match");
    MDB_stat stats;
    parity_check(mdb_stat(txn, db, &stats), "stat");
    parity_require(stats.ms_entries == image.entries, "stat counts logical records");
    return image;
}
void parity_print_image(const char *label, unsigned database, struct parity_image image) {
    printf("%s db=%u count=%" PRIu64 " forward=%016" PRIx64 " reverse=%016" PRIx64 "\n",
           label, database, image.entries, image.forward, image.reverse);
}
void parity_dump(MDB_txn *txn, MDB_dbi db, unsigned database) {
    MDB_cursor *cursor;
    parity_check(mdb_cursor_open(txn, db, &cursor), "dump cursor_open");
    MDB_val key = {0, NULL}, value = {0, NULL};
    int result = mdb_cursor_get(cursor, &key, &value, MDB_FIRST);
    while (!result) {
        printf("record db=%u ", database); hex(key); putchar('='); hex(value); putchar('\n');
        result = mdb_cursor_get(cursor, &key, &value, MDB_NEXT);
    }
    parity_require(result == MDB_NOTFOUND, "dump ends at MDB_NOTFOUND");
    mdb_cursor_close(cursor);
}
