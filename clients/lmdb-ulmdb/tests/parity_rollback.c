/* Unchanged LMDB 1.0 application. Every rollback immediately follows its
   commit; transaction IDs are cached before commit. No consecutive rollback. */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
static void need(int b,const char *s){if(!b){fprintf(stderr,"rollback oracle: %s\n",s);exit(1);}}
static void ok(int c,const char*s){if(c){fprintf(stderr,"%s: %d %s\n",s,c,mdb_strerror(c));exit(1);}}
static MDB_val text(const char*s){MDB_val v={strlen(s),(void*)s};return v;}
static void put(MDB_txn*t,MDB_dbi d,const char*v){MDB_val k=text("k"),x=text(v);ok(mdb_put(t,d,&k,&x,0),"put");}
static size_t head(MDB_env*e){MDB_envinfo i;ok(mdb_env_info(e,&i),"info");return i.me_last_txnid;}
static void get(MDB_txn*t,MDB_dbi d,const char*label,const char*want){MDB_val k=text("k"),v;int c=mdb_get(t,d,&k,&v);printf("%s=%d",label,c);if(!c){printf(" value=%.*s",(int)v.mv_size,(char*)v.mv_data);if(want)need(v.mv_size==strlen(want)&&!memcmp(v.mv_data,want,v.mv_size),label);}putchar('\n');if(want)ok(c,label);}
static unsigned calls;
static int reverse(const MDB_val*a,const MDB_val*b){calls++;size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;int c=memcmp(a->mv_data,b->mv_data,n);return c?-c:(a->mv_size<b->mv_size)-(a->mv_size>b->mv_size);}
static void openenv(MDB_env**e,const char*p,unsigned f){ok(mdb_env_create(e),"create");ok(mdb_env_set_maxdbs(*e,8),"maxdbs");ok(mdb_env_set_mapsize(*e,16u*1024u*1024u),"mapsize");ok(mdb_env_open(*e,p,f,0600),"open");}
static void scenario(const char*p,const char*mode,unsigned flags){
 need(!mkdir(p,0700)||errno==EEXIST,"scenario directory");MDB_env*e;MDB_txn*t,*old=NULL;MDB_dbi d,extra=0;MDB_cursor*cursor=NULL;MDB_val borrowed={0};openenv(&e,p,flags);
 int first=!strcmp(mode,"first"),created=!strcmp(mode,"created"),dropped=!strcmp(mode,"dropped"),custom=!strcmp(mode,"callback"),reader=!strcmp(mode,"reader");
 unsigned n=!strcmp(mode,"head3")?3:!strcmp(mode,"head4")?4:!strcmp(mode,"head5")?5:first?1:2;
 size_t id=0;
 for(unsigned i=1;i<=n;i++){
  if(reader&&i==n){ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&old),"old begin");MDB_val k=text("k");ok(mdb_get(old,d,&k,&borrowed),"borrow");ok(mdb_cursor_open(old,d,&cursor),"old cursor");MDB_val v;ok(mdb_cursor_get(cursor,&k,&v,MDB_SET),"position old cursor");}
  ok(mdb_txn_begin(e,NULL,0,&t),"begin");ok(mdb_dbi_open(t,"named",MDB_CREATE,&d),"dbi");
  if(custom&&i==1)ok(mdb_set_compare(t,d,reverse),"set compare");
  char value[16];snprintf(value,sizeof value,"v%u",i);if(!(dropped&&i==n))put(t,d,value);
  if(created&&i==n){ok(mdb_dbi_open(t,"future",MDB_CREATE|MDB_REVERSEKEY,&extra),"future create");put(t,extra,"future");}
  if(dropped&&i==n){ok(mdb_drop(t,d,1),"drop named");d=0;}
  id=mdb_txn_id(t);if(!strcmp(mode,"prepared")&&i==n)ok(mdb_txn_prepare(t),"prepare");
  ok(mdb_txn_commit(t),"commit");t=NULL;
  if(i<n)need(head(e)==i,"seed head");
 }
 /* This is the first environment operation after the final commit. */
 size_t supplied=!strcmp(mode,"zero")?0:!strcmp(mode,"wrong-old")?id-1:!strcmp(mode,"wrong-future")?id+1:id;
 int rc=mdb_env_rollback(e,supplied);printf("id=%zu supplied=%zu rollback=%d\n",id,supplied,rc);
 int succeeds=!first&&strcmp(mode,"zero")&&strcmp(mode,"wrong-old")&&strcmp(mode,"wrong-future");need(rc==(succeeds?0:MDB_CANT_ROLLBACK),"rollback result");
 size_t expected=succeeds?n-1:n;printf("head=%zu\n",head(e));need(head(e)==expected,"selected head");
 if(reader){need(borrowed.mv_size==2&&!memcmp(borrowed.mv_data,"v1",2),"old borrowed bytes");get(old,d,"old.get","v1");MDB_val k,v;ok(mdb_cursor_get(cursor,&k,&v,MDB_GET_CURRENT),"old current");need(v.mv_size==2&&!memcmp(v.mv_data,"v1",2),"old cursor bytes");puts("old.borrow=preserved old.cursor=preserved");mdb_cursor_close(cursor);mdb_txn_abort(old);old=NULL;}
 ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&t),"inspect");
 if(dropped)ok(mdb_dbi_open(t,"named",0,&d),"restored proper reopen");
 char want[16];snprintf(want,sizeof want,"v%zu",expected);get(t,d,"selected.get",want);
 if(created){unsigned f=0;int c=mdb_dbi_flags(t,extra,&f);printf("undone.alias.flags=%d",c);if(!c)printf(" flags=%u",f);putchar('\n');get(t,extra,"undone.alias.get",NULL);}
 if(custom){MDB_val a=text("a"),b=text("z");calls=0;int c=mdb_cmp(t,d,&a,&b);printf("callback.sign=%d calls=%u\n",(c>0)-(c<0),calls);need(c>0&&calls==1,"callback survives");}
 mdb_txn_abort(t);t=NULL;
 if(!strcmp(mode,"reuse")){
  ok(mdb_txn_begin(e,NULL,0,&t),"reuse begin");put(t,d,"replacement");id=mdb_txn_id(t);ok(mdb_txn_commit(t),"reuse commit");
  rc=mdb_env_rollback(e,id);printf("reuse.id=%zu rollback=%d\n",id,rc);ok(rc,"reuse rollback");need(head(e)==expected,"reuse restored head");
 }
 mdb_env_close(e);openenv(&e,p,flags);need(head(e)==expected,"reopen head");ok(mdb_txn_begin(e,NULL,MDB_RDONLY,&t),"reopen read");ok(mdb_dbi_open(t,"named",0,&d),"reopen named");get(t,d,"reopen.get",want);mdb_txn_abort(t);
 ok(mdb_txn_begin(e,NULL,0,&t),"next writer");printf("next.writer=%zu\n",mdb_txn_id(t));need(mdb_txn_id(t)==expected+1,"next writer id");mdb_txn_abort(t);mdb_env_close(e);
}
int main(int argc,char**argv){if(argc!=2&&argc!=4)return 2;if(mkdir(argv[1],0700)&&errno!=EEXIST)return 2;puts(mdb_strerror(MDB_CANT_ROLLBACK));const char*modes[]={"head2","head3","head4","head5","first","zero","wrong-old","wrong-future","prepared","created","dropped","callback","reader","reuse"};unsigned f[]={0,MDB_NOTLS,MDB_WRITEMAP,MDB_NOLOCK};for(unsigned j=0;j<4;j++)for(unsigned i=0;i<sizeof modes/sizeof*modes;i++){if(argc==4&&(strcmp(argv[2],modes[i])||strtoul(argv[3],NULL,10)!=f[j]))continue;/* Pinned WRITEMAP drop commits assert before rollback; retained separately. */if(f[j]==MDB_WRITEMAP&&!strcmp(modes[i],"dropped"))continue;char p[4096];snprintf(p,sizeof p,"%s/%s-%u",argv[1],modes[i],j);printf("case=%s flags=%u\n",modes[i],f[j]);scenario(p,modes[i],f[j]);}return 0;}
