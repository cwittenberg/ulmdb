/* Adapter-only native capability boundary. No LMDB prefix parity assertion. */
#define _GNU_SOURCE
#include <ulmdb.h>
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>
#define NEED(x) do { if (!(x)) {fprintf(stderr,"typed boundary line%d: %s\n",__LINE__,#x);exit(2);} } while(0)
#define OK(x) do {int rc_=(x);if(rc_){fprintf(stderr,"typed boundary line%d %s: %d\n",__LINE__,#x,rc_);exit(2);}} while(0)
static MDB_val text(const char*s){return(MDB_val){strlen(s),(void*)s};}
static void eq(MDB_val v,const char*s){NEED(v.mv_size==strlen(s)&&!memcmp(v.mv_data,s,v.mv_size));}
static MDB_env*open_env(const char*path){MDB_env*e;OK(mdb_env_create(&e));OK(mdb_env_set_maxdbs(e,16));OK(mdb_env_open(e,path,MDB_NOTLS,0600));return e;}
static MDB_txn*begin(MDB_env*e,unsigned flags){MDB_txn*t;OK(mdb_txn_begin(e,NULL,flags,&t));return t;}
static MDB_dbi named(MDB_txn*t,const char*name,unsigned kind,unsigned flags){MDB_dbi d;OK(ulmdb_dbi_open(t,name,kind,flags,&d));return d;}
static void put(MDB_txn*t,MDB_dbi d,const char*s){MDB_val k=text("k"),v=text(s);OK(mdb_put(t,d,&k,&v,0));}
static MDB_val get(MDB_txn*t,MDB_dbi d,const char*s){MDB_val k=text("k"),v;OK(mdb_get(t,d,&k,&v));eq(v,s);return v;}
static MDB_val prefix(MDB_txn*t,MDB_dbi d,const char*cidr,const char*s){MDB_val v;OK(ulmdb_prefix_get(t,d,cidr,&v));eq(v,s);return v;}
static void values(MDB_txn*t,MDB_dbi*k){MDB_val v=text("B");put(t,k[0],"B");put(t,k[1],"B");OK(ulmdb_prefix_put(t,k[2],"192.0.2.0/24",&v));OK(ulmdb_prefix_put(t,k[3],"2001:db8::/32",&v));}
static const char*names[]={"kv","dups","routes4","routes6"};
static const unsigned kinds[]={ULMDB_KV,ULMDB_DUPLICATES,ULMDB_IPV4,ULMDB_IPV6};
static void peer(const char*path){MDB_env*e=open_env(path);MDB_txn*t=begin(e,0);MDB_dbi d[4];for(unsigned i=0;i<4;i++){MDB_dbi old=named(t,names[i],kinds[i],0);OK(mdb_drop(t,old,1));/* Closed output is never used again. */d[i]=named(t,names[i],kinds[i],MDB_CREATE);}values(t,d);OK(mdb_txn_commit(t));mdb_env_close(e);}
int main(int argc,char**argv){setvbuf(stdout,NULL,_IONBF,0);Dl_info info;NEED(dladdr((void*)mdb_env_open,&info)&&info.dli_fname);fprintf(stderr,"PROVIDER %s\n",info.dli_fname);if(argc==3&&!strcmp(argv[1],"--peer")){peer(argv[2]);return 0;}NEED(argc==2);NEED(!mkdir(argv[1],0700));MDB_env*e=open_env(argv[1]);MDB_txn*t=begin(e,0);MDB_dbi d[4];for(unsigned i=0;i<4;i++)d[i]=named(t,names[i],kinds[i],MDB_CREATE);MDB_val a=text("A");put(t,d[0],"A");put(t,d[1],"A");OK(ulmdb_prefix_put(t,d[2],"192.0.2.0/24",&a));OK(ulmdb_prefix_put(t,d[3],"2001:db8::/32",&a));MDB_dbi bad;NEED(ulmdb_dbi_open(t,"kv",ULMDB_DUPLICATES,0,&bad)==MDB_INCOMPATIBLE);NEED(ulmdb_dbi_open(t,"dups",ULMDB_KV,0,&bad)==MDB_INCOMPATIBLE);NEED(ulmdb_dbi_open(t,"routes4",ULMDB_IPV6,0,&bad)==MDB_INCOMPATIBLE);OK(mdb_txn_commit(t));
 MDB_txn*old=begin(e,MDB_RDONLY);MDB_val borrowed_kv=get(old,d[0],"A"),borrowed_dup=get(old,d[1],"A"),borrowed_v4=prefix(old,d[2],"192.0.2.0/24","A"),borrowed_v6=prefix(old,d[3],"2001:db8::/32","A");fflush(stdout);pid_t pid=fork();NEED(pid>=0);if(!pid){execl(argv[0],argv[0],"--peer",argv[1],(char*)NULL);_exit(127);}int status;NEED(waitpid(pid,&status,0)==pid&&WIFEXITED(status)&&!WEXITSTATUS(status));
 t=begin(e,MDB_RDONLY);get(t,d[0],"B");get(t,d[1],"B");MDB_val v;NEED(ulmdb_prefix_get(t,d[2],"192.0.2.0/24",&v)==MDB_BAD_DBI);NEED(ulmdb_prefix_get(t,d[3],"2001:db8::/32",&v)==MDB_BAD_DBI);mdb_txn_abort(t);eq(borrowed_kv,"A");eq(borrowed_dup,"A");eq(borrowed_v4,"A");eq(borrowed_v6,"A");prefix(old,d[2],"192.0.2.0/24","A");prefix(old,d[3],"2001:db8::/32","A");mdb_txn_abort(old);mdb_dbi_close(e,d[2]);mdb_dbi_close(e,d[3]);/* Reopen only after every old transaction ends. */
 t=begin(e,MDB_RDONLY);d[2]=named(t,"routes4",ULMDB_IPV4,0);d[3]=named(t,"routes6",ULMDB_IPV6,0);prefix(t,d[2],"192.0.2.0/24","B");prefix(t,d[3],"2001:db8::/32","B");mdb_txn_abort(t);mdb_env_close(e);puts("typed-alias-boundaries-ok kind-open/refreshed-kv-dups/stale-prefix/old-borrows/proper-reopen");return 0;}
