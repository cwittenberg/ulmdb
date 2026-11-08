#define _GNU_SOURCE
#include <lmdb.h>
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#define OK(call) do { int rc=(call); if(rc){fprintf(stderr,"%s: %d\n",#call,rc);exit(1);} } while(0)
#define CHECK(x) do { if(!(x)){fprintf(stderr,"check: %s\n",#x);exit(1);} } while(0)
static MDB_env *opened(const char *path,unsigned flags) {
    MDB_env *env; OK(mdb_env_create(&env));OK(mdb_env_set_mapsize(env,64u*1024u*1024u));
    OK(mdb_env_open(env,path,MDB_NOTLS|flags,0600));return env;
}
static MDB_dbi seed(MDB_env *env) {
    MDB_txn *w;MDB_dbi db;MDB_val k={3,"key"},v={3,"one"};
    OK(mdb_txn_begin(env,NULL,0,&w));OK(mdb_dbi_open(w,NULL,0,&db));OK(mdb_put(w,db,&k,&v,0));OK(mdb_txn_commit(w));return db;
}
static void put(MDB_env *env,MDB_dbi db,const char *value) {
    MDB_txn *w;MDB_val k={3,"key"},v={strlen(value),(void*)value};
    OK(mdb_txn_begin(env,NULL,0,&w));OK(mdb_put(w,db,&k,&v,0));OK(mdb_txn_commit(w));
}
static void expect(MDB_env *env,const char *value,size_t id) {
    MDB_txn *r;MDB_dbi db;MDB_val k={3,"key"},v;
    OK(mdb_txn_begin(env,NULL,MDB_RDONLY,&r));OK(mdb_dbi_open(r,NULL,0,&db));
    CHECK(mdb_txn_id(r)==id);OK(mdb_get(r,db,&k,&v));CHECK(v.mv_size==strlen(value)&&!memcmp(v.mv_data,value,v.mv_size));OK(mdb_txn_commit(r));
}
int main(int argc,char **argv) {
    CHECK(argc==5);CHECK(!mkdir(argv[1],0700));CHECK(!mkdir(argv[2],0700));
    MDB_env *source=opened(argv[1],0);MDB_dbi db=seed(source);
    OK(mdb_env_copy(source,argv[2]));put(source,db,"two");put(source,db,"three");
    unsigned before_flags,after_flags;
    OK(mdb_env_get_flags(source,&before_flags));CHECK(!(before_flags&MDB_NOSUBDIR));
    OK(mdb_env_incr_dump(source,argv[3],1));
    OK(mdb_env_get_flags(source,&after_flags));CHECK(after_flags==(before_flags|MDB_NOSUBDIR));
    int fd=open(argv[3],O_RDONLY);CHECK(fd>=0);
    MDB_env *target=opened(argv[2],MDB_WRITEMAP);OK(mdb_env_incr_loadfd(target,fd));CHECK(fcntl(fd,F_GETFD)>=0);CHECK(lseek(fd,0,SEEK_CUR)>0);CHECK(!close(fd));mdb_env_close(target);
    target=opened(argv[2],0);expect(target,"three",3);mdb_env_close(target);
    /* Reopen without NOSUBDIR so the FD check cannot pass just because the
       earlier path call already set the bit. Re-export MAIN in this owner. */
    mdb_env_close(source);source=opened(argv[1],0);
    MDB_txn *r;OK(mdb_txn_begin(source,NULL,MDB_RDONLY,&r));OK(mdb_dbi_open(r,NULL,0,&db));OK(mdb_txn_commit(r));
    put(source,db,"four");fd=open(argv[4],O_RDWR|O_CREAT|O_EXCL,0600);CHECK(fd>=0);
    OK(mdb_env_get_flags(source,&before_flags));CHECK(!(before_flags&MDB_NOSUBDIR));
    OK(mdb_env_incr_dumpfd(source,fd,3));CHECK(fcntl(fd,F_GETFD)>=0);CHECK(lseek(fd,0,SEEK_SET)==0);
    OK(mdb_env_get_flags(source,&after_flags));CHECK(after_flags==before_flags);
    target=opened(argv[2],MDB_WRITEMAP);OK(mdb_env_incr_loadfd(target,fd));CHECK(fcntl(fd,F_GETFD)>=0);CHECK(!close(fd));mdb_env_close(target);
    target=opened(argv[2],0);expect(target,"four",4);mdb_env_close(target);
    /* Existing directory forces destination-open failure before dump work.
       The source keeps its original storage binding despite the new flag. */
    OK(mdb_env_get_flags(source,&before_flags));CHECK(!(before_flags&MDB_NOSUBDIR));
    CHECK(mdb_env_incr_dump(source,argv[1],3)==EEXIST);
    OK(mdb_env_get_flags(source,&after_flags));CHECK(after_flags==(before_flags|MDB_NOSUBDIR));
    expect(source,"four",4);mdb_env_close(source);
    puts("incremental-ok: base1->3->4, path+fd, caller-fd retained, reopen-data exact");return 0;
}
