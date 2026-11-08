/* Native snapshot copies through the unchanged LMDB API. */
#include <lmdb.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#define REQUIRE(x) do{if(!(x)){fprintf(stderr,"%s:%d: %s\n",__FILE__,__LINE__,#x);exit(1);}}while(0)
#define OK(x) do{int rc_=(x);if(rc_){fprintf(stderr,"%s:%d: %s returned %d (%s)\n",__FILE__,__LINE__,#x,rc_,mdb_strerror(rc_));exit(1);}}while(0)
static MDB_val text(const char*s){MDB_val v={strlen(s),(void*)s};return v;}
static MDB_env *open_env(const char*path,unsigned flags){MDB_env*e;OK(mdb_env_create(&e));OK(mdb_env_set_maxdbs(e,8));OK(mdb_env_open(e,path,flags|MDB_NOTLS,0600));return e;}
/* Export the source DBI before overlapping transactions begin. A transaction
 * using mdb_dbi_open must end before another transaction calls that function. */
static MDB_dbi seed(MDB_env*env){MDB_txn*w;MDB_dbi db;MDB_val k=text("key"),v=text("old");OK(mdb_txn_begin(env,NULL,0,&w));OK(mdb_dbi_open(w,"data",MDB_CREATE,&db));OK(mdb_put(w,db,&k,&v,0));OK(mdb_txn_commit(w));return db;}
static MDB_dbi open_data(MDB_env*env){MDB_txn*r;MDB_dbi db;OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&r));OK(mdb_dbi_open(r,"data",0,&db));OK(mdb_txn_commit(r));return db;}
static void put(MDB_env*env,MDB_dbi db,const char*value){MDB_txn*w;MDB_val k=text("key"),v=text(value);OK(mdb_txn_begin(env,NULL,0,&w));OK(mdb_put(w,db,&k,&v,0));OK(mdb_txn_commit(w));}
static void expect(MDB_env*env,MDB_dbi db,const char*value){MDB_txn*r;MDB_val k=text("key"),v;OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&r));OK(mdb_get(r,db,&k,&v));REQUIRE(v.mv_size==strlen(value));REQUIRE(!memcmp(v.mv_data,value,v.mv_size));mdb_txn_abort(r);}
int main(int argc,char**argv){
    REQUIRE(argc==5);MDB_env*source=open_env(argv[1],0);MDB_dbi db=seed(source);
    MDB_txn*old;MDB_val k=text("key"),v;OK(mdb_txn_begin(source,NULL,MDB_RDONLY,&old));
    put(source,db,"copied");
    MDB_txn*private_write; MDB_val private_key=text("key"), private_value=text("uncommitted");
    OK(mdb_txn_begin(source,NULL,0,&private_write)); OK(mdb_put(private_write,db,&private_key,&private_value,0));
    REQUIRE(mkdir(argv[2],0700)==0);OK(mdb_env_copy(source,argv[2]));OK(mdb_env_copy2(source,argv[3],MDB_CP_COMPACT));
    int fd=open(argv[4],O_WRONLY|O_CREAT|O_EXCL,0600);REQUIRE(fd>=0);OK(mdb_env_copyfd2(source,fd,MDB_CP_COMPACT));REQUIRE(fcntl(fd,F_GETFD)>=0);REQUIRE(close(fd)==0);
    mdb_txn_abort(private_write); expect(source,db,"copied");
    put(source,db,"latest");OK(mdb_get(old,db,&k,&v));REQUIRE(v.mv_size==3&&!memcmp(v.mv_data,"old",3));mdb_txn_abort(old);
    MDB_env*copy=open_env(argv[2],MDB_RDONLY);MDB_dbi copied=open_data(copy);expect(copy,copied,"copied");mdb_env_close(copy);
    copy=open_env(argv[3],0);copied=open_data(copy);expect(copy,copied,"copied");put(copy,copied,"independent");mdb_env_close(copy);expect(source,db,"latest");
    copy=open_env(argv[4],MDB_NOSUBDIR|MDB_RDONLY);copied=open_data(copy);expect(copy,copied,"copied");mdb_env_close(copy);
    REQUIRE(mdb_env_copy(source,argv[2])!=0);REQUIRE(mdb_env_copy2(source,argv[3],2)!=0);mdb_env_close(source);puts("copies-ok");return 0;
}
