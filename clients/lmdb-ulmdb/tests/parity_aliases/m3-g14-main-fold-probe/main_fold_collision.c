/* Proper ordinary MVCC/catalog lookup under an application MAIN comparator.
 * Every environment installs fold BEFORE any catalog/data access. No PREV. */
#define _GNU_SOURCE
#include <lmdb.h>
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>
static unsigned calls;
static int errors;
static void need(int b,const char*s){if(!b){fprintf(stderr,"fold probe fatal: %s\n",s);exit(2);}}
static void ok(int rc,const char*s){if(rc){fprintf(stderr,"%s: %d %s\n",s,rc,mdb_strerror(rc));exit(2);}}
static int fold(const MDB_val*a,const MDB_val*b){calls++;size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;for(size_t i=0;i<n;i++){unsigned char x=((unsigned char*)a->mv_data)[i],y=((unsigned char*)b->mv_data)[i];if(x>='A'&&x<='Z')x=(unsigned char)(x+('a'-'A'));if(y>='A'&&y<='Z')y=(unsigned char)(y+('a'-'A'));if(x!=y)return(x>y)-(x<y);}return(a->mv_size>b->mv_size)-(a->mv_size<b->mv_size);}
static MDB_val val(const char*s){return(MDB_val){strlen(s),(void*)s};}
static MDB_env*env(const char*p,unsigned flags){MDB_env*e;ok(mdb_env_create(&e),"create");ok(mdb_env_set_maxdbs(e,8),"maxdbs");ok(mdb_env_set_mapsize(e,16u*1024u*1024u),"mapsize");ok(mdb_env_open(e,p,flags|MDB_NOTLS,0600),"open");return e;}
static MDB_txn*begin(MDB_env*e,unsigned flags){MDB_txn*t;ok(mdb_txn_begin(e,NULL,flags,&t),"begin");return t;}
static MDB_dbi main_fold(MDB_txn*t){MDB_dbi d;ok(mdb_dbi_open(t,NULL,0,&d),"main open before comparator");ok(mdb_set_compare(t,d,fold),"MAIN comparator before catalog/data");return d;}
static void expected(int rc,int want,const char*label){printf("%s=%d\n",label,rc);if(rc!=want){fprintf(stderr,"fold probe mismatch %s got=%d expected=%d\n",label,rc,want);errors++;}}
static void peer(const char*path){MDB_env*e=env(path,0);MDB_txn*t=begin(e,0);MDB_dbi main=main_fold(t),x;ok(mdb_dbi_open(t,"x",0,&x),"peer x open");ok(mdb_drop(t,x,1),"peer removes only its own named DBI");/* x is closed and never referenced again. */char upper[]="X";MDB_val k={sizeof upper-(MDB_VERSION_MAJOR<1),upper},v=val("ordinary");ok(mdb_put(t,main,&k,&v,0),"ordinary uppercase row after named deletion");ok(mdb_txn_commit(t),"peer commit");mdb_env_close(e);}
int main(int argc,char**argv){setvbuf(stdout,NULL,_IONBF,0);Dl_info provider;need(dladdr((void*)mdb_env_open,&provider)&&provider.dli_fname,"provider");fprintf(stderr,"PROVIDER %s\n",provider.dli_fname);if(argc==3&&!strcmp(argv[1],"--peer")){peer(argv[2]);return 0;}if(argc!=2)return 2;need(!mkdir(argv[1],0700),"fresh directory");puts("scenario=G14-main-fold-collision");MDB_env*e=env(argv[1],0);MDB_txn*t=begin(e,0);(void)main_fold(t);MDB_dbi x;ok(mdb_dbi_open(t,"x",MDB_CREATE,&x),"seed x");MDB_val k=val("k"),v=val("A");ok(mdb_put(t,x,&k,&v,0),"seed named data");ok(mdb_txn_commit(t),"seed commit");MDB_txn*old=begin(e,MDB_RDONLY);MDB_val borrowed;ok(mdb_get(old,x,&k,&borrowed),"old A borrow");MDB_cursor*c;ok(mdb_cursor_open(old,x,&c),"old A cursor");ok(mdb_cursor_get(c,&k,&v,MDB_SET),"position old A");fflush(stdout);pid_t p=fork();need(p>=0,"fork peer");if(!p){execl(argv[0],argv[0],"--peer",argv[1],(char*)NULL);_exit(127);}int status;need(waitpid(p,&status,0)==p&&WIFEXITED(status)&&!WEXITSTATUS(status),"peer completed");t=begin(e,MDB_RDONLY);MDB_dbi alias;ok(mdb_dbi_open(t,"x",0,&alias),"cached alias open");need(alias==x,"parent alias unchanged");unsigned flags=~0u;int rc=mdb_dbi_flags(t,x,&flags);expected(rc,0,"cached.flags.rc");if(!rc)need(flags==0,"cached ordinary flags");calls=0;k=val("k");rc=mdb_get(t,x,&k,&v);unsigned used=calls;expected(rc,MDB_INCOMPATIBLE,"cached.get");printf("cached.MAIN.fold.used=%u\n",used>0);if(!used){fputs("fold probe mismatch cached comparator not invoked\n",stderr);errors++;}mdb_txn_abort(t);need(borrowed.mv_size==1&&*(char*)borrowed.mv_data=='A',"old borrowed A survives peer");ok(mdb_cursor_get(c,&k,&v,MDB_GET_CURRENT),"old cursor after peer");need(v.mv_size==1&&*(char*)v.mv_data=='A',"old cursor A bytes");mdb_cursor_close(c);mdb_txn_abort(old);mdb_env_close(e);
 e=env(argv[1],MDB_RDONLY);t=begin(e,MDB_RDONLY);(void)main_fold(t);calls=0;MDB_dbi unused;rc=mdb_dbi_open(t,"x",0,&unused);used=calls;expected(rc,MDB_INCOMPATIBLE,"reopen.lowercase");printf("reopen.MAIN.fold.used=%u\n",used>0);if(!used){fputs("fold probe mismatch reopen comparator not invoked\n",stderr);errors++;}rc=mdb_dbi_open(t,"X",0,&unused);expected(rc,MDB_INCOMPATIBLE,"reopen.uppercase.control");mdb_txn_abort(t);mdb_env_close(e);printf("fold-collision-complete errors=%d\n",errors);return errors?1:0;}
