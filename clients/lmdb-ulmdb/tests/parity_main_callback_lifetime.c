#include <lmdb.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <sys/stat.h>
static unsigned calls;static int errors;
static void ok(int r,const char*s){if(r){fprintf(stderr,"%s %d\n",s,r);exit(2);}}
static int fold(const MDB_val*a,const MDB_val*b){calls++;size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;for(size_t i=0;i<n;i++){unsigned char x=((unsigned char*)a->mv_data)[i],y=((unsigned char*)b->mv_data)[i];if(x>='A'&&x<='Z')x+=32;if(y>='A'&&y<='Z')y+=32;if(x!=y)return(x>y)-(x<y);}return(a->mv_size>b->mv_size)-(a->mv_size<b->mv_size);}
static MDB_val val(const char*s){return(MDB_val){strlen(s),(void*)s};}
static void compare(MDB_txn*t,MDB_dbi d,const char*label){MDB_val a=val("X"),b=val("x");calls=0;int c=mdb_cmp(t,d,&a,&b);printf("%s.equal=%d callback=%d\n",label,c==0,calls>0);if(c||calls!=1)errors++;}
int main(int argc,char**argv){if(argc!=2)return 2;if(mkdir(argv[1],0700)&&errno!=EEXIST)return 2;MDB_env*e;ok(mdb_env_create(&e),"create");ok(mdb_env_set_mapsize(e,16u*1024u*1024u),"mapsize");ok(mdb_env_open(e,argv[1],MDB_NOTLS,0600),"open");MDB_txn*t;MDB_dbi d;ok(mdb_txn_begin(e,0,MDB_RDONLY,&t),"initial read");ok(mdb_dbi_open(t,0,0,&d),"initial MAIN");ok(mdb_set_compare(t,d,fold),"MAIN comparator before any data");compare(t,d,"before-first-writer");ok(mdb_txn_commit(t),"initial read commit");ok(mdb_txn_begin(e,0,0,&t),"first writer");compare(t,d,"after-first-writer-begin");MDB_val k=val("X"),v=val("one");ok(mdb_put(t,d,&k,&v,0),"first put");k=val("x");v=val("two");ok(mdb_put(t,d,&k,&v,0),"equal-key overwrite");compare(t,d,"after-first-data");ok(mdb_txn_commit(t),"first commit");ok(mdb_txn_begin(e,0,MDB_RDONLY,&t),"final read");compare(t,d,"after-first-commit");MDB_stat s;ok(mdb_stat(t,d,&s),"stat");printf("entries=%zu\n",s.ms_entries);if(s.ms_entries!=1)errors++;for(int i=0;i<2;i++){k=val(i?"x":"X");int c=mdb_get(t,d,&k,&v);printf("get%d=%d value=%.*s\n",i,c,c?0:(int)v.mv_size,c?"":(char*)v.mv_data);if(c||v.mv_size!=3||memcmp(v.mv_data,"two",3))errors++;}mdb_txn_abort(t);mdb_env_close(e);printf("main-callback-complete errors=%d\n",errors);return errors?1:0;}
