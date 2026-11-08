#include <lmdb.h>
MDB_crypto_funcs *MDB_crypto(void) {
    static MDB_crypto_funcs functions = {0,0,0,32,16,32};
    return &functions;
}
MDB_crypto_funcs *custom_hooks(void) { return MDB_crypto(); }
