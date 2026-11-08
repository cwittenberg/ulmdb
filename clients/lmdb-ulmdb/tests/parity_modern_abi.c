/* Check the supported LMDB 1.0 declarations against the actual pinned header. */
#include <lmdb.h>
#include <stdio.h>

typedef int (*txn_flags_function)(MDB_txn *, unsigned int *);
typedef int (*cursor_is_db_function)(MDB_cursor *);
_Static_assert(_Generic(&mdb_txn_flags, txn_flags_function: 1, default: 0),
               "mdb_txn_flags signature");
_Static_assert(_Generic(&mdb_cursor_is_db, cursor_is_db_function: 1, default: 0),
               "mdb_cursor_is_db signature");
int main(void) {
    printf("mdb_size_t %zu %zu\n", sizeof(mdb_size_t), _Alignof(mdb_size_t));
    printf("MDB_DBIS_BUSY %d\n", MDB_DBIS_BUSY);
    printf("MDB_IS_READONLY %d\n", MDB_IS_READONLY);
    puts("mdb_txn_flags signature=int(MDB_txn*,unsigned int*)");
    puts("mdb_cursor_is_db signature=int(MDB_cursor*)");
    return 0;
}
