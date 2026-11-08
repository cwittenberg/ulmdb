/* Unchanged LMDB 1.0 application: prepare lifecycle and physical dirtiness. */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
static void need(int yes,const char *what) { if(!yes) { fprintf(stderr,"prepare oracle: %s\n",what); exit(1); } }
static void ok(int code,const char *what) { if(code) { fprintf(stderr,"%s: %d %s\n",what,code,mdb_strerror(code)); exit(1); } }
static MDB_val text(const char *s) { MDB_val value={strlen(s),(void *)s}; return value; }
static int put(MDB_txn *txn,MDB_dbi db,const char *k,const char *v,unsigned flags) { MDB_val key=text(k),value=text(v); return mdb_put(txn,db,&key,&value,flags); }
static size_t head(MDB_env *env) { MDB_envinfo info; ok(mdb_env_info(env,&info),"envinfo"); return info.me_last_txnid; }
static void expect(int code,int expected,const char *label) { printf("%s=%d\n",label,code); need(code==expected,label); }
static void get(MDB_txn *txn,MDB_dbi db,const char *value) { MDB_val key=text("k"),data; ok(mdb_get(txn,db,&key,&data),"get"); need(data.mv_size==strlen(value)&&!memcmp(data.mv_data,value,data.mv_size),"get bytes"); }
int main(int argc,char **argv) {
    if(argc!=5) return 2;
    puts(mdb_strerror(MDB_TXN_PENDING));
    const char *mode=argv[2]; unsigned flags=atoi(argv[3])?MDB_NOTLS:0; int abort_final=atoi(argv[4]);
    int fresh=!strcmp(mode,"clean-fresh")||!strcmp(mode,"empty-main-clear")||!strcmp(mode,"main-flags-new");
    int same_main_flags=!strcmp(mode,"main-flags-identical");
    int main_only=!strcmp(mode,"main-identical")||same_main_flags||!strcmp(mode,"main-flags-new");
    int empty_named=!strcmp(mode,"empty-named-clear");
    if(mkdir(argv[1],0700)&&errno!=EEXIST) return 2;
    MDB_env *env; MDB_txn *txn,*child=NULL; MDB_dbi main,named=0,dups=0;
    ok(mdb_env_create(&env),"env create");ok(mdb_env_set_maxdbs(env,16),"maxdbs");ok(mdb_env_set_maxreaders(env,1),"maxreaders");
    ok(mdb_env_set_mapsize(env,16u*1024u*1024u),"mapsize");ok(mdb_env_open(env,argv[1],flags,0600),"env open");
    if(!fresh) {
        ok(mdb_txn_begin(env,NULL,0,&txn),"seed begin");ok(mdb_dbi_open(txn,NULL,same_main_flags?MDB_REVERSEKEY:0,&main),"seed main");
        if(main_only) {if(!same_main_flags)ok(put(txn,main,"k","old",0),"seed main put");}
        else {
            ok(mdb_dbi_open(txn,"named",MDB_CREATE,&named),"seed named");
            if(!empty_named) ok(put(txn,named,"k","old",0),"seed named put");
            ok(mdb_dbi_open(txn,"duplicates",MDB_CREATE|MDB_DUPSORT,&dups),"seed duplicates");
            ok(put(txn,dups,"k","a",0),"seed dup a");ok(put(txn,dups,"k","b",0),"seed dup b");
        }
        ok(mdb_txn_commit(txn),"seed commit");txn=NULL;
    }
    size_t base=head(env);int dirty=1,readonly=!strcmp(mode,"readonly");
    ok(mdb_txn_begin(env,NULL,readonly?MDB_RDONLY:0,&txn),"begin");ok(mdb_dbi_open(txn,NULL,0,&main),"main open");
    size_t id=mdb_txn_id(txn);int code=0,expected=0;
    if(!strcmp(mode,"clean-fresh")||!strcmp(mode,"clean")||readonly) dirty=0;
    else if(!strcmp(mode,"dirty-kv")) code=put(txn,named,"k","new",0);
    else if(!strcmp(mode,"kv-identical")) code=put(txn,named,"k","old",0);
    else if(same_main_flags) {code=mdb_dbi_open(txn,NULL,MDB_REVERSEKEY,&main);dirty=0;}
    else if(!strcmp(mode,"main-flags-new")) code=mdb_dbi_open(txn,NULL,MDB_REVERSEKEY,&main);
    else if(main_only) code=put(txn,main,"k","old",0);
    else if(!strcmp(mode,"dup-equivalent")) code=put(txn,dups,"k","a",0);
    else if(!strcmp(mode,"dup-nodupdata")) {code=put(txn,dups,"k","a",MDB_NODUPDATA);expected=MDB_KEYEXIST;}
    else if(!strcmp(mode,"dup-appenddup")) {code=put(txn,dups,"k","a",MDB_APPENDDUP);expected=MDB_KEYEXIST;}
    else if(!strcmp(mode,"kv-nooverwrite")) {code=put(txn,named,"k","old",MDB_NOOVERWRITE);expected=MDB_KEYEXIST;dirty=0;}
    else if(!strcmp(mode,"dup-nooverwrite")) {code=put(txn,dups,"k","a",MDB_NOOVERWRITE);expected=MDB_KEYEXIST;dirty=0;}
    else if(!strcmp(mode,"kv-append-rejected")) {code=put(txn,named,"a","old",MDB_APPEND);expected=MDB_KEYEXIST;dirty=0;}
    else if(!strcmp(mode,"empty-main-clear")) {code=mdb_drop(txn,main,0);dirty=0;}
    else if(empty_named) code=mdb_drop(txn,named,0);
    else if(!strcmp(mode,"reserve")) {MDB_val key=text("k"),value={3,NULL};code=mdb_put(txn,named,&key,&value,MDB_RESERVE);if(!code)memcpy(value.mv_data,"new",3);}
    else if(!strcmp(mode,"created-dbi")) {MDB_dbi created;ok(mdb_dbi_open(txn,"created",MDB_CREATE,&created),"created dbi");code=put(txn,created,"k","new",0);}
    else if(!strncmp(mode,"nested-",7)) {
        ok(mdb_txn_begin(env,txn,0,&child),"child begin");
        dirty=strcmp(mode,"nested-clean")!=0&&strcmp(mode,"nested-abort")!=0;
        if(strcmp(mode,"nested-clean")) ok(put(child,named,"k","new",0),"child put");
        if(!strcmp(mode,"nested-abort")) {mdb_txn_abort(child);child=NULL;puts("child.abort=done");}
        else {ok(mdb_txn_prepare(child),"child prepare consumes");child=NULL;puts("child.prepare=consumed");}
        get(txn,named,dirty?"new":"old");
    } else need(0,"mode");
    expect(code,expected,"operation");
    MDB_cursor *cursor=NULL;ok(mdb_cursor_open(txn,main,&cursor),"cursor before prepare");
    expect(mdb_txn_prepare(txn),0,"prepare");
    printf("mode=%s notls=%d base=%zu id=%zu dirty=%d\n",mode,!!flags,base,id,dirty);
    if(readonly) {txn=NULL;mdb_cursor_close(cursor);cursor=NULL;puts("readonly=consumed");}
    else if(!dirty) {txn=NULL;cursor=NULL;puts("clean=consumed");}
    else {
        cursor=NULL; /* Upstream frees every write cursor during prepare. */
        need(mdb_txn_id(txn)==id&&mdb_txn_env(txn)==env,"pending identity");
        unsigned public_flags=~0u;ok(mdb_txn_flags(txn,&public_flags),"pending flags");need(!public_flags,"public flags only");
        expect(mdb_txn_prepare(txn),MDB_TXN_PENDING,"prepare.repeat");
        MDB_val key=text("k"),value=text("blocked");MDB_stat stat;MDB_dbi ignored;MDB_cursor *blocked=NULL;
        expect(mdb_get(txn,main,&key,&value),MDB_BAD_TXN,"pending.get");
        expect(mdb_put(txn,main,&key,&value,0),MDB_BAD_TXN,"pending.put");
        expect(mdb_del(txn,main,&key,NULL),MDB_BAD_TXN,"pending.del");
        expect(mdb_stat(txn,main,&stat),MDB_BAD_TXN,"pending.stat");
        expect(mdb_dbi_open(txn,NULL,0,&ignored),MDB_BAD_TXN,"pending.open");
        expect(mdb_drop(txn,main,0),main_only?MDB_BAD_TXN:MDB_DBIS_BUSY,"pending.drop-main");
        if(!main_only) expect(mdb_drop(txn,named,0),MDB_BAD_TXN,"pending.drop-named");
        expect(mdb_cursor_open(txn,main,&blocked),MDB_BAD_TXN,"pending.cursor");need(!blocked,"no pending cursor");
        expect(mdb_txn_begin(env,txn,0,&child),MDB_BAD_TXN,"pending.child");need(!child,"no child");
        expect(mdb_txn_begin(env,txn,MDB_RDONLY,&child),EINVAL,"pending.readonly-child");need(!child,"no readonly child");
        need(head(env)==base,"prepare has not published");
        MDB_txn *reader;ok(mdb_txn_begin(env,NULL,MDB_RDONLY,&reader),"pending independent reader");need(mdb_txn_id(reader)==base,"reader sees base");
        if(!main_only&&!empty_named) get(reader,named,"old");
        mdb_txn_abort(reader);
        if(abort_final) {mdb_txn_abort(txn);puts("final=abort");}else {ok(mdb_txn_commit(txn),"prepared final commit");puts("final=commit");}txn=NULL;
    }
    size_t committed=base+(!readonly&&dirty&&!abort_final);need(head(env)==committed,"head after final");
    ok(mdb_txn_begin(env,NULL,MDB_RDONLY,&txn),"fresh reader");need(mdb_txn_id(txn)==committed,"fresh reader id");
    if(!fresh&&!main_only&&!empty_named) get(txn,named,dirty&&!abort_final&&(!strcmp(mode,"dirty-kv")||!strcmp(mode,"reserve")||!strncmp(mode,"nested-",7))?"new":"old");
    if(!strcmp(mode,"created-dbi")) {MDB_dbi created;code=mdb_dbi_open(txn,"created",0,&created);expect(code,abort_final?MDB_NOTFOUND:0,"created.after-final");if(!code)get(txn,created,"new");}
    mdb_txn_abort(txn);txn=NULL;
    ok(mdb_txn_begin(env,NULL,0,&txn),"next writer");need(mdb_txn_id(txn)==committed+1,"next writer id");mdb_txn_abort(txn);txn=NULL;
    printf("head=%zu next-writer=%zu\n",committed,committed+1);
    mdb_env_close(env);return 0;
}
