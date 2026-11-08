#include <lmdb.h>
#include <stdio.h>
#include <stdlib.h>
#define CHECK(x) do { if(!(x)){fprintf(stderr,"check: %s\n",#x);exit(1);} } while(0)
int main(int argc, char **argv) {
    CHECK(argc==2);MDB_crypto_funcs *functions=NULL;char *error=NULL;
    void *module=mdb_modload(argv[1],NULL,&functions,&error);
    CHECK(module&&functions&&functions->mcf_keysize==32&&functions->mcf_esumsize==16);
    mdb_modunload(module);functions=NULL;
    module=mdb_modload(argv[1],"custom_hooks",&functions,&error);CHECK(module&&functions);mdb_modunload(module);
    module=mdb_modload(argv[1],"missing_hooks",&functions,&error);CHECK(!module&&error);
    module=mdb_modload("/no/such/ulmdb-crypto-module",NULL,&functions,&error);CHECK(!module&&error);
    puts("crypto-module-ok: default/custom hooks, unload, missing module/symbol");return 0;
}
