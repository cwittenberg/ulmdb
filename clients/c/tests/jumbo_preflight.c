#include "lmdb.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
static void ok(int rc){if(rc){fprintf(stderr,"rc=%d\n",rc);abort();}}
int main(int argc,char **argv){assert(argc==2);MDB_env*e;MDB_txn*t;MDB_dbi db;ok(mdb_env_create(&e));ok(mdb_env_set_mapsize(e,1024*1024));ok(mdb_env_open(e,argv[1],MDB_NOTLS,0600));ok(mdb_txn_begin(e,0,0,&t));ok(mdb_dbi_open(t,0,0,&db));MDB_val k={1,"x"},v={1,"A"};ok(mdb_put(t,db,&k,&v,0));ok(mdb_txn_commit(t));ok(mdb_txn_begin(e,0,0,&t));
 v=(MDB_val){257u*1024u*1024u,0};int rc=mdb_put(t,db,&k,&v,MDB_RESERVE|MDB_NOOVERWRITE);fprintf(stderr,"NOOVERWRITE=%d\n",rc);assert(rc==MDB_KEYEXIST);assert(v.mv_size==1);
 k=(MDB_val){1,"a"};v=(MDB_val){257u*1024u*1024u,0};rc=mdb_put(t,db,&k,&v,MDB_RESERVE|MDB_APPEND);fprintf(stderr,"APPEND=%d\n",rc);assert(rc==MDB_KEYEXIST);
 k=(MDB_val){1,"z"};v=(MDB_val){257u*1024u*1024u,0};rc=mdb_put(t,db,&k,&v,MDB_RESERVE);fprintf(stderr,"MAPFULL=%d\n",rc);assert(rc==MDB_MAP_FULL);
 assert(mdb_txn_commit(t)==MDB_BAD_TXN);ok(mdb_txn_begin(e,0,0,&t));
 k=(MDB_val){1,"y"};v=(MDB_val){1,"B"};ok(mdb_put(t,db,&k,&v,0));ok(mdb_txn_commit(t));mdb_env_close(e);puts("jumbo preflight/flag error ordering/abort-only MAPFULL/fresh writer=ok");return 0;}
