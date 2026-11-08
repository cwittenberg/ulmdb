#define _GNU_SOURCE
#include <lmdb.h>
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
static void ok(int rc,const char *where){if(rc){fprintf(stderr,"ERROR where=%s rc=%d text=%s\n",where,rc,mdb_strerror(rc));exit(2);}}
static void provider(void){Dl_info d; if(!dladdr((void*)mdb_env_open,&d)||!d.dli_fname)exit(3);fprintf(stderr,"PROVIDER path=%s version=%s\n",d.dli_fname,mdb_version(NULL,NULL,NULL));fflush(stderr);}
static int opening(const char *path,unsigned flags,MDB_env **out){ok(mdb_env_create(out),"create");ok(mdb_env_set_mapsize(*out,16u*1024u*1024u),"mapsize");ok(mdb_env_set_maxdbs(*out,8),"maxdbs");ok(mdb_env_set_maxreaders(*out,32),"readers");printf("OPENING flags=%u\n",flags);fflush(stdout);int rc=mdb_env_open(*out,path,flags|MDB_NOTLS,0600);printf("OPEN_RESULT rc=%d\n",rc);fflush(stdout);if(!rc){MDB_txn*bootstrap=NULL;MDB_dbi main=0;ok(mdb_txn_begin(*out,NULL,MDB_RDONLY,&bootstrap),"bootstrap-main-begin");ok(mdb_dbi_open(bootstrap,NULL,0,&main),"bootstrap-main-open");printf("BOOTSTRAP_MAIN id=%zu dbi=%u\n",(size_t)mdb_txn_id(bootstrap),(unsigned)main);ok(mdb_txn_commit(bootstrap),"bootstrap-main-end");}return rc;}
static void data(MDB_txn *t,MDB_dbi d,const char *key,const char *label){MDB_val k={strlen(key),(void*)key},v={0,NULL};int rc=mdb_get(t,d,&k,&v);printf("VALUE label=%s dbi=%u key=%s rc=%d bytes=",label,(unsigned)d,key,rc);if(!rc){for(size_t i=0;i<v.mv_size;i++)printf("%02x",((unsigned char*)v.mv_data)[i]);}puts("");}
static void view(MDB_txn *t,const char *label){printf("TXN label=%s id=%zu\n",label,(size_t)mdb_txn_id(t));data(t,1,"k",label);data(t,1,"first",label);data(t,1,"second",label);}
static MDB_txn *begin(MDB_env *e,unsigned flags){MDB_txn*t=NULL;ok(mdb_txn_begin(e,NULL,flags,&t),"begin");return t;}
static void info(MDB_env *e,const char *label){unsigned flags=0;MDB_envinfo i;ok(mdb_env_get_flags(e,&flags),"flags");ok(mdb_env_info(e,&i),"info");printf("INFO label=%s id=%zu flags=%u\n",label,(size_t)i.me_last_txnid,flags);}
static void readenv(MDB_env*e,const char *label){info(e,label);MDB_txn*t=begin(e,MDB_RDONLY);view(t,label);mdb_txn_abort(t);}
static void put(MDB_txn*t,const char*value,const char*tag){MDB_val k={1,(void*)"k"},v={strlen(value),(void*)value};ok(mdb_put(t,1,&k,&v,0),"put-main");if(tag&&strcmp(tag,"-")){MDB_val tk={strlen(tag),(void*)tag};ok(mdb_put(t,1,&tk,&v,0),"put-tag");}}
int main(int argc,char**argv){setvbuf(stdout,NULL,_IOLBF,0);provider();
#ifdef MDB_PREVSNAPSHOT
 if(argc==2&&!strcmp(argv[1],"caps")){printf("CAPS previous=%u readonly=%u nolock=%u compact=%u\n",MDB_PREVSNAPSHOT,MDB_RDONLY,MDB_NOLOCK,MDB_CP_COMPACT);return 0;}
#else
 if(argc==2&&!strcmp(argv[1],"caps")){puts("CAPS previous=unsupported");return 0;}
#endif
 if(argc<4){return 4;}
 const char *path=argv[2];MDB_env*e=NULL;
 if(!strcmp(argv[1],"seed")){if(mkdir(path,0700)){perror("mkdir");return 4;}ok(opening(path,0,&e),"seed-open");int n=atoi(argv[3]);for(int i=1;i<=n;i++){char v[32];snprintf(v,sizeof v,"seed%d",i);MDB_txn*t=begin(e,0);put(t,v,NULL);ok(mdb_txn_commit(t),"seed-commit");}readenv(e,"seed-final");mdb_env_close(e);return 0;}
 if(strcmp(argv[1],"agent")){return 4;}
 unsigned flags=(unsigned)strtoul(argv[3],NULL,0);int rc=opening(path,flags,&e);if(rc){mdb_env_close(e);return 0;}puts("READY");char line[2048],cmd[64],arg[1024],tag[64];while(fgets(line,sizeof line,stdin)){cmd[0]=arg[0]=tag[0]=0;int fields=sscanf(line,"%63s %1023s %63s",cmd,arg,tag);if(fields<1)continue;
 if(!strcmp(cmd,"close"))break;
 if(!strcmp(cmd,"read")){readenv(e,fields>1?arg:"read");}
 else if(!strcmp(cmd,"info")){info(e,fields>1?arg:"info");}
 else if(!strcmp(cmd,"clean")){MDB_txn*t=begin(e,0);view(t,"clean-base");ok(mdb_txn_commit(t),"clean-commit");info(e,"after-clean");}
 else if(!strcmp(cmd,"copy")){if(mkdir(arg,0700)){perror("copy-mkdir");return 4;}ok(mdb_env_copy2(e,arg,fields>2&&atoi(tag)?MDB_CP_COMPACT:0),"copy");puts("COPIED");}
 else if(!strcmp(cmd,"write")||!strcmp(cmd,"abort")||!strcmp(cmd,"prepare-abort")||!strcmp(cmd,"prepare-write")||!strcmp(cmd,"write-rollback")){
  MDB_txn*t=begin(e,0);view(t,"writer-base");put(t,arg,fields>2?tag:NULL);size_t id=(size_t)mdb_txn_id(t);printf("WRITER id=%zu action=%s\n",id,cmd);
  if(!strncmp(cmd,"prepare-",8)){
#ifdef MDB_TXN_PENDING
   ok(mdb_txn_prepare(t),"prepare");info(e,"prepared");readenv(e,"reader-during-prepare");
#else
   fputs("prepare unavailable\n",stderr);mdb_txn_abort(t);return 5;
#endif
  }
  if(!strcmp(cmd,"abort")||!strcmp(cmd,"prepare-abort")){mdb_txn_abort(t);t=NULL;puts("ABORTED");}
  else {ok(mdb_txn_commit(t),"commit");t=NULL;
   if(!strcmp(cmd,"write-rollback")){
#ifdef MDB_CANT_ROLLBACK
    rc=mdb_env_rollback(e,id);printf("ROLLBACK cached-id=%zu rc=%d\n",id,rc);ok(rc,"rollback");
#else
    fputs("rollback unavailable\n",stderr);return 5;
#endif
   }
  }
  info(e,"after-action");
 }
 else {fprintf(stderr,"unknown command %s\n",cmd);return 4;}
 printf("DONE %s\n",cmd);
 }mdb_env_close(e);puts("CLOSED");return 0;}
