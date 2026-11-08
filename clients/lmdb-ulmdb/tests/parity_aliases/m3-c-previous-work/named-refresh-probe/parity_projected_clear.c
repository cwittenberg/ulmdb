/* Separate from the frozen four ordinary refresh journeys. */
#define main original_named_refresh_main
#include "parity_named_refresh.c"
#undef main
static void clear_case(const char *self,const char *path,int dirty){need(!mkdir(path,0700),"clear mkdir");MDB_env *e=open_env(path,MDB_NOTLS);MDB_txn *t=begin(e,0);MDB_dbi x=db(t,"x",MDB_CREATE);ok(mdb_set_compare(t,x,bytes_compare),"clear seed comparator");put(t,x,"k","A");ok(mdb_txn_commit(t),"clear A commit");fflush(stdout);pid_t p=fork();need(p>=0,"fork clear peer");if(!p){execl(self,self,"--peer",path,"0",(char*)NULL);_exit(127);}int status;need(waitpid(p,&status,0)==p&&WIFEXITED(status)&&!WEXITSTATUS(status),"clear peer");t=begin(e,0);MDB_dbi main=db(t,NULL,0);value(t,x,"k","B","projected.clear.loaded");comparator(t,x);if(dirty)put(t,x,"k","dirty");MDB_cursor *c;ok(mdb_cursor_open(t,x,&c),"clear named cursor");MDB_val k=val("k"),v;ok(mdb_cursor_get(c,&k,&v,MDB_SET),"clear position");int rc=mdb_drop(t,main,0);printf("projected.main.clear=%d\n",rc);
#if MDB_VERSION_MAJOR>=1
    need(rc==MDB_DBIS_BUSY,"modern valid logical aliases prevent clear");
#else
    need(!rc,"legacy clear success");
#endif
    value(t,x,"k",dirty?"dirty":"B","projected.clear.retained");ok(mdb_cursor_get(c,&k,&v,MDB_GET_CURRENT),"retained projected current");need(v.mv_size==strlen(dirty?"dirty":"B")&&!memcmp(v.mv_data,dirty?"dirty":"B",v.mv_size),"retained projected cursor bytes");flags(t,x,0,"projected.clear.flags");comparator(t,x);mdb_cursor_close(c);
#if MDB_VERSION_MAJOR>=1
    mdb_txn_abort(t);
#else
    if(!dirty){MDB_val key=val("new"),data=val("forbidden");rc=mdb_put(t,x,&key,&data,0);printf("projected.clear.clean.put=%d\n",rc);need(rc==MDB_NOTFOUND,"clean cached DBI cannot resurrect");value(t,x,"k","B","projected.clear.after-rejected-put");}ok(mdb_txn_commit(t),"clear parent commit");
#endif
    mdb_env_close(e);e=open_env(path,MDB_RDONLY|MDB_NOTLS);t=begin(e,MDB_RDONLY);MDB_dbi fresh;rc=mdb_dbi_open(t,"x",0,&fresh);printf("projected.clear.reopen=%d\n",rc);
#if MDB_VERSION_MAJOR>=1
    need(!rc,"modern aborted clear preserves named catalog");ok(mdb_set_compare(t,fresh,bytes_compare),"modern reopen comparator");value(t,fresh,"k","B","projected.clear.reopen.value");
#else
    if(dirty){need(!rc,"dirty named catalog reinserted");ok(mdb_set_compare(t,fresh,bytes_compare),"legacy dirty reopen comparator");value(t,fresh,"k","dirty","projected.clear.reopen.value");}else need(rc==MDB_NOTFOUND,"clean catalog absent durably");
#endif
    mdb_txn_abort(t);mdb_env_close(e);}
int main(int argc,char**argv){if(argc==4&&!strcmp(argv[1],"--peer")){change(argv[2],0);return 0;}if(argc!=2)return 2;need(!mkdir(argv[1],0700)||errno==EEXIST,"clear root");for(int dirty=0;dirty<2;dirty++){char path[4096];int n=snprintf(path,sizeof path,"%s/dirty%d",argv[1],dirty);need(n>0&&(size_t)n<sizeof path,"clear path");printf("scenario=G14-projected-main-clear-dirty%d\n",dirty);clear_case(argv[0],path,dirty);}puts("projected-clear-complete journeys=2");return 0;}
