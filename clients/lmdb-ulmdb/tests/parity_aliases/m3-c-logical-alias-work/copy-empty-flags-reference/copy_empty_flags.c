#define _GNU_SOURCE
#include <lmdb.h>
#include <dlfcn.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#define OK(x)do{int r=(x);if(r){fprintf(stderr,"line%d %s=%d\n",__LINE__,#x,r);exit(2);}}while(0)
#define NEED(x)do{if(!(x)){fprintf(stderr,"line%d %s\n",__LINE__,#x);exit(2);}}while(0)
static MDB_env*opening(const char*p,unsigned f){MDB_env*e;OK(mdb_env_create(&e));OK(mdb_env_set_mapsize(e,16*1024*1024));OK(mdb_env_set_maxdbs(e,8));OK(mdb_env_open(e,p,f|MDB_NOTLS,0600));return e;}
static MDB_txn*begin(MDB_env*e,unsigned f){MDB_txn*t;OK(mdb_txn_begin(e,NULL,f,&t));return t;}
static MDB_val val(const char*s){return(MDB_val){strlen(s),(void*)s};}
static void put_main(MDB_txn*t,MDB_dbi d,unsigned flags,const char*label){uint32_t integer=1;MDB_val k=(flags&MDB_INTEGERKEY)?(MDB_val){sizeof integer,&integer}:val("k"),v=val(label);OK(mdb_put(t,d,&k,&v,0));}
static void del_main(MDB_txn*t,MDB_dbi d,unsigned flags){uint32_t integer=1;MDB_val k=(flags&MDB_INTEGERKEY)?(MDB_val){sizeof integer,&integer}:val("k");OK(mdb_del(t,d,&k,NULL));}
static void show(MDB_env*e,const char*label){MDB_txn*t=begin(e,MDB_RDONLY);MDB_dbi main;OK(mdb_dbi_open(t,NULL,0,&main));unsigned flags=~0u;OK(mdb_dbi_flags(t,main,&flags));MDB_stat st;OK(mdb_stat(t,main,&st));printf("%s.id=%zu flags=%u entries=%zu\n",label,(size_t)mdb_txn_id(t),flags,st.ms_entries);uint32_t integer=1;MDB_val k=(flags&MDB_INTEGERKEY)?(MDB_val){sizeof integer,&integer}:val("k"),v;int rc=mdb_get(t,main,&k,&v);printf("%s.get=%d",label,rc);if(!rc){printf(" size=%zu bytes=",v.mv_size);for(size_t i=0;i<v.mv_size;i++)printf("%02x",((unsigned char*)v.mv_data)[i]);}puts("");MDB_dbi named;rc=mdb_dbi_open(t,"named",0,&named);printf("%s.named.open=%d",label,rc);if(!rc){OK(mdb_dbi_flags(t,named,&flags));printf(" flags=%u",flags);MDB_val nk=val("n"),nv;int nr=mdb_get(t,named,&nk,&nv);printf(" n.get=%d",nr);if(!nr){printf(" n.bytes=");for(size_t i=0;i<nv.mv_size;i++)printf("%02x",((unsigned char*)nv.mv_data)[i]);}}puts("");mdb_txn_abort(t);}
int main(int argc,char**argv){setvbuf(stdout,NULL,_IONBF,0);Dl_info di;NEED(dladdr((void*)mdb_env_open,&di)&&di.dli_fname);fprintf(stderr,"PROVIDER %s\n",di.dli_fname);NEED(argc==4);int mode=atoi(argv[1]),compact=atoi(argv[2]);char src[4096],dst[4096];NEED(!mkdir(argv[3],0700));NEED(snprintf(src,sizeof src,"%s/source",argv[3])>0);NEED(snprintf(dst,sizeof dst,"%s/destination",argv[3])>0);NEED(!mkdir(src,0700));NEED(!mkdir(dst,0700));MDB_env*e=opening(src,0);unsigned flags=(mode==1||mode==3)?MDB_INTEGERKEY:mode==8?MDB_REVERSEKEY:0;
 if(mode){MDB_txn*t=begin(e,0);MDB_dbi main;OK(mdb_dbi_open(t,NULL,flags,&main));if(mode==2||mode==3||mode==6)put_main(t,main,flags,mode==6?"":"value");if(mode==4||mode==5||mode==7){MDB_dbi d;OK(mdb_dbi_open(t,"named",MDB_CREATE|(mode==7?(MDB_DUPSORT|MDB_DUPFIXED):0),&d));if(mode==5){MDB_val k=val("n"),v=val("v");OK(mdb_put(t,d,&k,&v,0));}}OK(mdb_txn_commit(t));if(mode==2||mode==3||mode==5){t=begin(e,0);OK(mdb_dbi_open(t,NULL,0,&main));if(mode==5){MDB_dbi d;OK(mdb_dbi_open(t,"named",0,&d));OK(mdb_drop(t,d,1));/* closed d never reused */}else del_main(t,main,flags);OK(mdb_txn_commit(t));}}
 printf("mode=%d compact=%d\n",mode,compact);show(e,"source");OK(mdb_env_copy2(e,dst,compact?MDB_CP_COMPACT:0));mdb_env_close(e);e=opening(dst,MDB_RDONLY);show(e,"copied");mdb_env_close(e);
#ifdef MDB_PREVSNAPSHOT
 e=opening(dst,MDB_RDONLY|MDB_PREVSNAPSHOT);show(e,"previous");mdb_env_close(e);
#else
 puts("previous.unsupported=1");
#endif
 e=opening(dst,0);MDB_txn*t=begin(e,0);MDB_dbi main;OK(mdb_dbi_open(t,NULL,0,&main));OK(mdb_dbi_flags(t,main,&flags));printf("fresh.writer.id=%zu\n",(size_t)mdb_txn_id(t));put_main(t,main,flags,"after");OK(mdb_txn_commit(t));mdb_env_close(e);puts("copy-empty-flags-observation-complete");return 0;}
