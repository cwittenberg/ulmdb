/* A second unchanged process installs A while globally exported B DBIs and a
 * NOTLS B cursor remain live. No DBI is used after an explicit close. */
#define main original_previous_named_main
#include "../named-probe/parity_previous_named.c"
#undef main
#include <sys/wait.h>
#include <unistd.h>

static void setter(const char *path) {
    MDB_env *e=open_env(path,MDB_RDONLY|MDB_PREVSNAPSHOT|MDB_NOTLS);
    MDB_txn *t=begin(e,MDB_RDONLY);MDB_dbi stable=db(t,"stable",0);
    ok(mdb_set_compare(t,stable,bytes_compare),"setter comparator");
    value(t,stable,"k","old","setter.A");mdb_txn_abort(t);mdb_env_close(e);
}
static void shared_named(const char *self,const char *path,int held) {
    need(!mkdir(path,0700),"shared mkdir");MDB_env *e=open_env(path,MDB_NOTLS);
    MDB_txn *t=begin(e,0);MDB_dbi stable=db(t,"stable",MDB_CREATE);
    ok(mdb_set_compare(t,stable,bytes_compare),"seed comparator");
    put(t,stable,"k","old");ok(mdb_txn_commit(t),"shared seed A");
    t=begin(e,0);put(t,stable,"k","new");MDB_dbi future=db(t,"future",MDB_CREATE|MDB_REVERSEKEY);
    put(t,future,"k","future");ok(mdb_txn_commit(t),"shared seed B");
    MDB_txn *old=NULL;MDB_cursor *cursor=NULL;MDB_val borrowed={0};
    if(held){old=begin(e,MDB_RDONLY);MDB_val k=val("k"),v;ok(mdb_get(old,stable,&k,&borrowed),"borrow B");ok(mdb_cursor_open(old,future,&cursor),"B cursor");ok(mdb_cursor_get(cursor,&k,&v,MDB_SET),"position B");need(v.mv_size==6&&!memcmp(v.mv_data,"future",6),"B cursor bytes");}
    pid_t pid=fork();need(pid>=0,"fork setter");if(pid==0){execl(self,self,"--setter",path,(char*)NULL);_exit(127);}int status;need(waitpid(pid,&status,0)==pid&&WIFEXITED(status)&&!WEXITSTATUS(status),"setter process");
    t=begin(e,MDB_RDONLY);need(mdb_txn_id(t)==1,"new reader A id");MDB_dbi a=db(t,"stable",MDB_DUPSORT),b=db(t,"future",MDB_DUPSORT);
    need(a==stable&&b==future,"B alias identities retained");flags(t,a,0,"shared.A.stable.flags");flags(t,b,MDB_REVERSEKEY,"shared.A.future.flags");value(t,a,"k","old","shared.A.stable");MDB_val absent_key=val("k"),absent_value;int absent_rc=mdb_get(t,b,&absent_key,&absent_value);printf("shared.A.future=%d\n",absent_rc);need(absent_rc==MDB_BAD_DBI,"B-only stale alias against nonempty A MAIN");MDB_cursor *missing_cursor;ok(mdb_cursor_open(t,b,&missing_cursor),"missing named cursor open still succeeds");int cursor_rc=mdb_cursor_get(missing_cursor,&absent_key,&absent_value,MDB_FIRST);printf("shared.A.future.cursor=%d\n",cursor_rc);need(cursor_rc==MDB_BAD_DBI,"missing named cursor actual load");mdb_cursor_close(missing_cursor);value(t,a,"k","old","shared.A.after-error");comparator(t,a);mdb_txn_abort(t);
    if(held){need(borrowed.mv_size==3&&!memcmp(borrowed.mv_data,"new",3),"borrowed B survived A installation");MDB_val k,v;ok(mdb_cursor_get(cursor,&k,&v,MDB_GET_CURRENT),"held B current");need(v.mv_size==6&&!memcmp(v.mv_data,"future",6),"held B cursor survived");value(old,stable,"k","new","shared.held.B");mdb_cursor_close(cursor);mdb_txn_abort(old);}
    t=begin(e,0);MDB_val put_key=val("k"),put_value=val("forbidden");int put_rc=mdb_put(t,future,&put_key,&put_value,0);printf("shared.writer.future.put=%d\n",put_rc);need(put_rc==MDB_BAD_DBI,"missing named writer error");value(t,stable,"k","old","shared.writer.after-error");mdb_txn_abort(t);
    /* All readers using the B-only alias ended before close. It is never used again. */
    mdb_dbi_close(e,future);
    t=begin(e,0);value(t,stable,"k","old","shared.writer.A");MDB_dbi branch=db(t,"branch",MDB_CREATE|MDB_DUPSORT);put(t,branch,"k","a");put(t,branch,"k","b");put(t,stable,"k","branch");need(mdb_txn_id(t)==2,"branch reused ID");ok(mdb_txn_commit(t),"shared branch commit");
    t=begin(e,MDB_RDONLY);value(t,stable,"k","branch","shared.final.stable");branch=db(t,"branch",0);duplicates(t,branch,"ab","shared.final.dups");flags(t,branch,MDB_DUPSORT,"shared.final.flags");missing(t,"future","shared.final.future");mdb_txn_abort(t);mdb_env_close(e);
}
int main(int argc,char **argv){if(argc==3&&!strcmp(argv[1],"--setter")){setter(argv[2]);return 0;}if(argc!=2)return 2;need(!mkdir(argv[1],0700)||errno==EEXIST,"shared root");for(int held=0;held<2;held++){char path[4096];int n=snprintf(path,sizeof path,"%s/held%d",argv[1],held);need(n>0&&(size_t)n<sizeof path,"shared path");printf("scenario=G03-shared-named-held%d\n",held);shared_named(argv[0],path,held);}puts("previous-shared-named-complete journeys=2");return 0;}
