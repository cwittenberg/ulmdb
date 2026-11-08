/* Public-C operator commands. No private MDB layouts or native engine headers. */
#include "operator.h"
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#ifdef _WIN32
#include "operator_windows.h"
#else
#include <unistd.h>
#endif
static void usage(FILE*f){fputs("usage: ulmdb-tool dump [--lmdb] [-s DATABASE] ENV\n       ulmdb-tool load [--mapsize BYTES] ENV < dump\n       ulmdb-tool stat|check [--json] ENV\n       ulmdb-tool readers [--clear] [--json] ENV\n       ulmdb-tool get [-s DATABASE] ENV KEY_HEX\n       ulmdb-tool copy [--compact] ENV DESTINATION\n       ulmdb-tool delta-dump ENV BASE_TRANSACTION > delta\n       ulmdb-tool delta-load ENV < delta\nDefault dump is binary-safe across database names; --lmdb emits VERSION=3 bytevalue.\nNative deltas require an exact physical base and the LMDB 1.0 ABI profile.\n",f);}
int tool_open(const char*path,unsigned flags,size_t mapsize,size_t pagesize,MDB_env**out){
 MDB_env*e=NULL;int rc=mdb_env_create(&e);if(rc)return rc;
 if((rc=mdb_env_set_maxdbs(e,TOOL_MAX_DBS)))goto fail;
 if(mapsize&&(rc=mdb_env_set_mapsize(e,mapsize)))goto fail;
 if(pagesize!=0&&pagesize!=4096&&(rc=mdb_env_set_pagesize(e,pagesize)))goto fail;
 if((rc=mdb_env_open(e,path,flags|MDB_NOTLS,0600)))goto fail;
 *out=e;return 0;
fail:mdb_env_close(e);return rc;
}
static int name_order(const void*a,const void*b){const struct tool_database*x=a,*y=b;if(!x->name||!y->name)return (x->name!=NULL)-(y->name!=NULL);size_t n=x->length<y->length?x->length:y->length;int c=n?memcmp(x->name,y->name,n):0;return c?c:(x->length>y->length)-(x->length<y->length);}
void tool_free_databases(struct tool_database*dbs,size_t n){for(size_t i=0;i<n;i++)free(dbs[i].name);free(dbs);}
int tool_databases(MDB_txn*t,struct tool_database**out,size_t*count){
 MDB_cursor*c=NULL;MDB_dbi main;MDB_val k,v;size_t n=0,cap=16;int rc=mdb_dbi_open(t,NULL,0,&main);if(rc)return rc;
 struct tool_database*dbs=calloc(cap,sizeof *dbs);if(!dbs)return ENOMEM;dbs[n++].dbi=main;
 if((rc=mdb_cursor_open(t,main,&c)))goto fail;
 while((rc=mdb_cursor_get(c,&k,&v,MDB_NEXT))==0){
  if(!mdb_cursor_is_db(c))continue;
  size_t length=k.mv_size;const unsigned char*name=k.mv_data;if(length&&name[length-1]==0)length--;
  if(!k.mv_size||length>65536||memchr(name,0,length)){rc=EINVAL;goto fail;}
  if(n==TOOL_MAX_DBS+1u){rc=MDB_DBS_FULL;goto fail;}
  if(n==cap){size_t next=cap*2;struct tool_database*p=realloc(dbs,next*sizeof *p);if(!p){rc=ENOMEM;goto fail;}dbs=p;memset(dbs+cap,0,(next-cap)*sizeof *dbs);cap=next;}
  char*copy=malloc(length+1);if(!copy){rc=ENOMEM;goto fail;}memcpy(copy,name,length);copy[length]=0;dbs[n].name=copy;dbs[n].length=length;
  rc=mdb_dbi_open(t,copy,0,&dbs[n].dbi);if(rc){free(copy);dbs[n].name=NULL;goto fail;}n++;
 }
 if(rc!=MDB_NOTFOUND)goto fail;
 mdb_cursor_close(c);c=NULL;qsort(dbs,n,sizeof *dbs,name_order);
 for(size_t i=0;i<n;i++){if((rc=mdb_dbi_flags(t,dbs[i].dbi,&dbs[i].flags)))goto fail;if(dbs[i].flags&~TOOL_DB_FLAGS){rc=MDB_INCOMPATIBLE;goto fail;}}
 *out=dbs;*count=n;return 0;
fail:if(c)mdb_cursor_close(c);tool_free_databases(dbs,n);return rc;
}
#ifdef _WIN32
static int tool_main(int,char**);
int wmain(int count,wchar_t **wide){
 char **args=calloc((size_t)count+1,sizeof *args);if(!args)return 1;
 int result=1;for(int i=0;i<count;i++){args[i]=tool_windows_utf8(wide[i]);if(!args[i])goto done;}
 result=tool_main(count,args);
 done:for(int i=0;i<count;i++)free(args[i]);free(args);return result;
}
static int tool_main(int argc,char**argv){
#else
int main(int argc,char**argv){
#endif
#ifdef _WIN32
 int initialization=tool_platform_init();if(initialization){fprintf(stderr,"ulmdb-tool: binary standard streams: %d\n",initialization);return 1;}
#endif
 if(argc<3){usage(argc==2&&!strcmp(argv[1],"--help")?stdout:stderr);return argc==2&&!strcmp(argv[1],"--help")?0:2;}
 const char*command=argv[1],*name=NULL,*args[3];int lmdb=0,compact=0,json=0,clear=0,n=0;size_t mapsize=0;
 for(int i=2;i<argc;i++){
  if(!strcmp(argv[i],"--lmdb"))lmdb=1;
  else if(!strcmp(argv[i],"--compact"))compact=1;
  else if(!strcmp(argv[i],"--json"))json=1;
  else if(!strcmp(argv[i],"--clear"))clear=1;
  else if(!strcmp(argv[i],"-s")&&i+1<argc)name=argv[++i];
  else if(!strcmp(argv[i],"--mapsize")&&i+1<argc){char*end;errno=0;unsigned long long x=strtoull(argv[++i],&end,10);if(errno||!*argv[i]||argv[i][0]<'0'||argv[i][0]>'9'||*end||!x||x>SIZE_MAX){usage(stderr);return 2;}mapsize=(size_t)x;}
  else if(n<3)args[n++]=argv[i];else{usage(stderr);return 2;}
 }
 if(strcmp(command,"dump")&&strcmp(command,"load")&&strcmp(command,"stat")&&strcmp(command,"check")&&strcmp(command,"readers")&&strcmp(command,"get")&&strcmp(command,"copy")&&strcmp(command,"delta-dump")&&strcmp(command,"delta-load")){usage(stderr);return 2;}
 if((lmdb&&strcmp(command,"dump"))||(compact&&strcmp(command,"copy"))||(mapsize&&strcmp(command,"load"))||(name&&strcmp(command,"dump")&&strcmp(command,"get"))||(json&&strcmp(command,"stat")&&strcmp(command,"check")&&strcmp(command,"readers"))||(clear&&strcmp(command,"readers"))){usage(stderr);return 2;}
 int expected=!strcmp(command,"get")||!strcmp(command,"copy")||!strcmp(command,"delta-dump")?2:1;
 if(n!=expected){usage(stderr);return 2;}
 int rc=0;MDB_env*e=NULL;
 if(!strcmp(command,"load")){if(name||lmdb||compact){usage(stderr);return 2;}rc=tool_load(args[0],mapsize);goto done;}
 if(!strcmp(command,"delta-load")){
  /* A missing/empty destination is not an incremental base. Read-only admission
   * first ensures invalid input never initializes a new business environment. */
  if((rc=tool_open(args[0],MDB_RDONLY,0,0,&e)))goto done;
  mdb_env_close(e);e=NULL;
  if((rc=tool_open(args[0],MDB_WRITEMAP,0,0,&e)))goto done;
  #ifdef _WIN32
  mdb_filehandle_t input;if((rc=tool_stdio_handle(stdin,&input)))goto done;
  rc=mdb_env_incr_loadfd(e,input);goto done;
#else
  rc=mdb_env_incr_loadfd(e,STDIN_FILENO);goto done;
#endif
 }
 if((rc=tool_open(args[0],MDB_RDONLY,0,0,&e)))goto done;
 if(!strcmp(command,"dump"))rc=tool_dump(e,name,lmdb);
 else if(!strcmp(command,"stat"))rc=tool_inspect(e,0,json);
 else if(!strcmp(command,"check"))rc=tool_inspect(e,1,json);
 else if(!strcmp(command,"readers"))rc=tool_readers(e,clear,json);
 else if(!strcmp(command,"delta-dump")){
  char*end;errno=0;unsigned long long since=strtoull(args[1],&end,10);
  if(errno||!*args[1]||args[1][0]<'0'||args[1][0]>'9'||*end||!since||since>SIZE_MAX){usage(stderr);rc=EINVAL;goto done;}
  if(fflush(stdout)){rc=errno?errno:EIO;goto done;}
  #ifdef _WIN32
  mdb_filehandle_t output;if((rc=tool_stdio_handle(stdout,&output)))goto done;
  rc=mdb_env_incr_dumpfd(e,output,(size_t)since);
#else
  rc=mdb_env_incr_dumpfd(e,STDOUT_FILENO,(size_t)since);
#endif
 }
 else if(!strcmp(command,"copy"))rc=mdb_env_copy2(e,args[1],compact?MDB_CP_COMPACT:0);
 else if(!strcmp(command,"get")){
  MDB_txn*t=NULL;MDB_dbi db;struct tool_buffer key={0};MDB_val k,v;
  if(!(rc=tool_unhex(args[1],&key,(size_t)mdb_env_get_maxkeysize(e)))&&!(rc=mdb_txn_begin(e,NULL,MDB_RDONLY,&t))&&!(rc=mdb_dbi_open(t,name,0,&db))){k.mv_data=key.data;k.mv_size=key.size;rc=mdb_get(t,db,&k,&v);if(!rc){rc=tool_hex(stdout,v.mv_data,v.mv_size);if(!rc&&putchar('\n')==EOF)rc=EIO;}}
  if(t){mdb_txn_abort(t);}
  free(key.data);
 }else{usage(stderr);rc=EINVAL;}
 if(!rc&&(fflush(stdout)||ferror(stdout)))rc=errno?errno:EIO;
done:if(e)mdb_env_close(e);if(rc)fprintf(stderr,"ulmdb-tool %s: %d %s\n",command,rc,mdb_strerror(rc));return rc?1:0;
}
