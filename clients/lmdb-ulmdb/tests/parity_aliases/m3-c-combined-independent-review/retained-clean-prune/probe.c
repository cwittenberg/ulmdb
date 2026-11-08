#define _GNU_SOURCE
#include <lmdb.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#define OK(x) do{int r=(x);if(r){fprintf(stderr,"setup %s=%d\n",#x,r);exit(2);}}while(0)
static int fold(const MDB_val*a,const MDB_val*b){size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;for(size_t i=0;i<n;i++){unsigned x=((unsigned char*)a->mv_data)[i],y=((unsigned char*)b->mv_data)[i];if(x>='A'&&x<='Z')x+=32;if(y>='A'&&y<='Z')y+=32;if(x!=y)return(x>y)-(x<y);}return(a->mv_size>b->mv_size)-(a->mv_size<b->mv_size);}
static MDB_dbi main_db(MDB_txn*t){MDB_dbi d;OK(mdb_dbi_open(t,NULL,0,&d));OK(mdb_set_compare(t,d,fold));return d;}
int main(int argc,char**argv){setvbuf(stdout,NULL,_IONBF,0);if(argc!=2)return 2;Dl_info p;if(!dladdr((void*)mdb_env_open,&p))return 2;fprintf(stderr,"PROVIDER %s\n",p.dli_fname);if(mkdir(argv[1],0700))return 2;MDB_env*e;MDB_txn*t;MDB_dbi upper,lower,y;OK(mdb_env_create(&e));OK(mdb_env_set_mapsize(e,16*1024*1024));OK(mdb_env_set_maxdbs(e,12));OK(mdb_env_open(e,argv[1],MDB_NOTLS,0600));OK(mdb_txn_begin(e,NULL,0,&t));(void)main_db(t);OK(mdb_dbi_open(t,"X",MDB_CREATE,&upper));MDB_val k={1,"k"},v={1,"A"};OK(mdb_put(t,upper,&k,&v,0));OK(mdb_txn_commit(t));
OK(mdb_txn_begin(e,NULL,0,&t));MDB_dbi m=main_db(t);OK(mdb_dbi_open(t,"x",0,&lower));OK(mdb_dbi_open(t,"X",0,&upper));OK(mdb_get(t,lower,&k,&v));OK(mdb_get(t,upper,&k,&v));int rc=mdb_drop(t,m,0);printf("clear=%d\n",rc);if(rc){mdb_txn_abort(t);mdb_env_close(e);puts("boundary=clear-rejected");return 0;}
/* Both named aliases remain CLEAN. No persistent cursor is open. */
mdb_dbi_close(e,upper);MDB_dbi fresh=0;OK(mdb_dbi_open(t,"X",MDB_CREATE,&fresh));printf("freshX.get=%d\n",mdb_get(t,fresh,&k,&v));
mdb_dbi_close(e,lower); /* Clean OLD alias; never referenced again. */
OK(mdb_dbi_open(t,"Y",MDB_CREATE,&y));printf("freshslots.X=%u Y=%u\n",fresh,y);rc=mdb_txn_commit(t);printf("commit=%d\n",rc);mdb_env_close(e);puts("retained-clean-prune-complete");return 0;}
