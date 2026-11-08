/* Real LMDB1.0 source for operator migration: NULL MAIN differs from "". */
#include <lmdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static void check(int rc,const char *what){if(rc){fprintf(stderr,"%s: %d\n",what,rc);exit(1);}}
int main(int argc,char **argv){
 if(argc!=2)return 2;
 MDB_env *env;MDB_txn *txn;MDB_dbi main_db,empty,long_db;
 check(mdb_env_create(&env),"env");check(mdb_env_set_maxdbs(env,8),"maxdbs");
 check(mdb_env_set_mapsize(env,64u<<20),"mapsize");check(mdb_env_open(env,argv[1],MDB_NOTLS,0600),"open");
 check(mdb_txn_begin(env,NULL,0,&txn),"begin");check(mdb_dbi_open(txn,NULL,0,&main_db),"MAIN");
 check(mdb_dbi_open(txn,"",MDB_CREATE,&empty),"named-empty");
 char name[601];memset(name,'n',600);name[600]=0;
 check(mdb_dbi_open(txn,name,MDB_CREATE,&long_db),"named-long");
 if(main_db==empty||main_db==long_db||empty==long_db)return 3;
 MDB_val key={1,"k"},value={1,"M"};check(mdb_put(txn,main_db,&key,&value,0),"put MAIN");
 value.mv_data="E";check(mdb_put(txn,empty,&key,&value,0),"put empty");
 value.mv_data="L";check(mdb_put(txn,long_db,&key,&value,0),"put long");
 check(mdb_txn_commit(txn),"commit");mdb_env_close(env);return 0;
}
