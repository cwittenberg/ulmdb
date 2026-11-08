/* Modern source-observed pure metadata of dirty prepared owners only.
 * No data, callback mutation or DBI open occurs while pending. */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
static unsigned calls, failures;
static void ok(int rc,const char *label){if(rc){fprintf(stderr,"%s=%d\n",label,rc);exit(2);}}
static int fold(const MDB_val*a,const MDB_val*b){calls++;size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;for(size_t i=0;i<n;i++){unsigned char x=((unsigned char*)a->mv_data)[i],y=((unsigned char*)b->mv_data)[i];if(x>='A'&&x<='Z')x+=32;if(y>='A'&&y<='Z')y+=32;if(x!=y)return(x>y)-(x<y);}return(a->mv_size>b->mv_size)-(a->mv_size<b->mv_size);}
static MDB_val val(const char*s){return(MDB_val){strlen(s),(void*)s};}
static void compare(MDB_txn*t,MDB_dbi d,int dup,const char*label){MDB_val a=val("X"),b=val("x");calls=0;int rc=dup?mdb_dcmp(t,d,&a,&b):mdb_cmp(t,d,&a,&b);printf("%s.equal=%d callback=%u\n",label,rc==0,calls);if(rc||calls!=1)failures++;}
static void scenario(const char*base,unsigned dup,unsigned notls,unsigned commit){char path[4096];int n=snprintf(path,sizeof path,"%s/%u-%u-%u",base,dup,notls,commit);if(n<0||(size_t)n>=sizeof path||mkdir(path,0700))exit(2);printf("scenario=MAIN-%s-%s-%s\n",dup?"duplicate":"key",notls?"notls":"tls",commit?"commit":"abort");MDB_env*e;MDB_txn*t;MDB_dbi main;ok(mdb_env_create(&e),"create");ok(mdb_env_set_mapsize(e,16u*1024u*1024u),"mapsize");ok(mdb_env_open(e,path,notls?MDB_NOTLS:0,0600),"open");ok(mdb_txn_begin(e,NULL,0,&t),"write begin");ok(mdb_dbi_open(t,NULL,dup?MDB_DUPSORT:0,&main),"MAIN open");if(dup)ok(mdb_set_dupsort(t,main,fold),"MAIN duplicate comparator before data");else ok(mdb_set_compare(t,main,fold),"MAIN key comparator before data");compare(t,main,dup,"live.before_data");MDB_val k=val(dup?"k":"X"),v=val(dup?"X":"one");ok(mdb_put(t,main,&k,&v,0),"dirty put one");if(dup)v=val("x");else{k=val("x");v=val("two");}ok(mdb_put(t,main,&k,&v,0),"dirty put two");compare(t,main,dup,"live.after_data");mdb_size_t id=mdb_txn_id(t);ok(mdb_txn_prepare(t),"dirty prepare");printf("pending.env.same=%d id.same=%d\n",mdb_txn_env(t)==e,mdb_txn_id(t)==id);if(mdb_txn_env(t)!=e||mdb_txn_id(t)!=id)failures++;unsigned flags=~0u;int rc=mdb_dbi_flags(t,main,&flags);printf("pending.MAIN.flags.rc=%d value=%u\n",rc,flags);if(rc||flags!=(dup?MDB_DUPSORT:0))failures++;compare(t,main,dup,"pending.compare");if(commit){ok(mdb_txn_commit(t),"final commit");puts("finish=commit");}else{mdb_txn_abort(t);puts("finish=abort");}mdb_env_close(e);}
int main(int argc,char**argv){if(argc!=2)return 2;if(mkdir(argv[1],0700)&&errno!=EEXIST)return 2;for(unsigned dup=0;dup!=2;dup++)for(unsigned notls=0;notls!=2;notls++)for(unsigned commit=0;commit!=2;commit++)scenario(argv[1],dup,notls,commit);printf("pending-main-complete failures=%u\n",failures);return failures?1:0;}
