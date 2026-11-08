/* Deterministic byte-oriented differential workload, shared with its driver. */
#ifndef ULMDB_PARITY_WORKLOAD_DATA_H
#define ULMDB_PARITY_WORKLOAD_DATA_H
#include <lmdb.h>
#include <stdint.h>

struct parity_random { uint64_t state, step; };
struct parity_image { uint64_t forward, reverse, entries; };

uint32_t parity_random_next(struct parity_random *);
void parity_check(int, const char *);
void parity_require(int, const char *);
void parity_operation(MDB_txn *, MDB_dbi, MDB_dbi, struct parity_random *);
struct parity_image parity_image(MDB_txn *, MDB_dbi);
void parity_print_image(const char *, unsigned, struct parity_image);
void parity_dump(MDB_txn *, MDB_dbi, unsigned);
#endif
