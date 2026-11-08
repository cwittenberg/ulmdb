#define _GNU_SOURCE
#include <lmdb.h>
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>

/* Every MAIN open finishes before another open on that environment. Handles
 * exported by each seed commit are reused by later overlapping read/copy calls.
 * No comparator is changed after data access; these modes use their defaults. */
#define OK(x) do{int z=(x);if(z){fprintf(stderr,"FAIL line=%d rc=%d\n",__LINE__,z);return 2;}}while(0)
static uint32_t vals[4]={UINT32_C(0x02000001),UINT32_C(0x01000002),UINT32_C(0x00000003),UINT32_C(0x03000004)};
static void hex(const void *p,size_t n){const unsigned char *b=p;for(size_t i=0;i<n;i++)printf("%02x",b[i]);}
static int dump(const char *path,const char *label,int previous){
 MDB_env *e=NULL;MDB_txn *t=NULL;MDB_cursor *c=NULL;MDB_dbi d=0;unsigned f=0,ef=MDB_NOTLS|MDB_RDONLY;MDB_stat st;MDB_envinfo info;
#ifdef MDB_PREVSNAPSHOT
 if(previous)ef|=MDB_PREVSNAPSHOT;
#else
 if(previous){printf("UNAVAILABLE view=%s api=MDB_PREVSNAPSHOT\n",label);return 0;}
#endif
 OK(mdb_env_create(&e));OK(mdb_env_open(e,path,ef,0600));OK(mdb_txn_begin(e,NULL,MDB_RDONLY,&t));OK(mdb_dbi_open(t,NULL,0,&d));OK(mdb_dbi_flags(t,d,&f));OK(mdb_stat(t,d,&st));OK(mdb_env_info(e,&info));OK(mdb_cursor_open(t,d,&c));MDB_val k={0,NULL},v={0,NULL};int rc=mdb_cursor_get(c,&k,&v,MDB_FIRST);size_t count=0;if(!rc)OK(mdb_cursor_count(c,&count));
 printf("VIEW name=%s flags=%u entries=%zu first=%d first_count=%zu txn=%zu info=%zu order=",label,f,st.ms_entries,rc,count,mdb_txn_id(t),info.me_last_txnid);
 while(!rc){hex(k.mv_data,k.mv_size);printf(":");hex(v.mv_data,v.mv_size);printf(",");rc=mdb_cursor_get(c,&k,&v,MDB_NEXT);}
 printf(" end=%d\n",rc);if(rc!=MDB_NOTFOUND)return 3;mdb_cursor_close(c);mdb_txn_abort(t);mdb_env_close(e);return 0;
}
static int widths(const char *path,const char *label,int populated){
 MDB_env *e=NULL;MDB_txn *t=NULL;MDB_dbi d=0;unsigned f=0;OK(mdb_env_create(&e));OK(mdb_env_open(e,path,MDB_NOTLS,0600));OK(mdb_txn_begin(e,NULL,0,&t));OK(mdb_dbi_open(t,NULL,0,&d));OK(mdb_dbi_flags(t,d,&f));
 MDB_val k={1,(void *)"k"},good={sizeof(uint32_t),&vals[3]},z={1,(void *)"z"};
 /* All data items obey the fixed/integer width4 precondition. The separate
  * observationalv2 preserves upstream's invalid-size padding/truncation;
  * this permanent positive oracle never supplies an inconsistent width. */
 int first=mdb_put(t,d,&k,&good,0),second=mdb_put(t,d,&z,&good,0);if(first||second)return 3;
 printf("WIDTH name=%s flags=%u populated=%d first4=%d then4=%d\n",label,f,populated,first,second);
 OK(mdb_txn_commit(t));mdb_env_close(e);return dump(path,label,0);
}
int main(int argc,char **argv){
 if(argc!=4){return 2;}
 unsigned flags=MDB_DUPSORT;const char *variant=argv[2];int populated=atoi(argv[3]);
 if(!strcmp(variant,"fixed"))flags|=MDB_DUPFIXED;
 else if(!strcmp(variant,"integer"))flags|=MDB_DUPFIXED|MDB_INTEGERDUP;
 else if(!strcmp(variant,"reverse"))flags|=MDB_REVERSEDUP;
 else if(!strcmp(variant,"fixed-reverse"))flags|=MDB_DUPFIXED|MDB_REVERSEDUP;
 else if(strcmp(variant,"dupsort"))return 2;
 char source[4096],raw[4096],compact[4096],stream[4096];
 if(snprintf(source,sizeof source,"%s/source",argv[1])<0||snprintf(raw,sizeof raw,"%s/raw",argv[1])<0||snprintf(compact,sizeof compact,"%s/compact",argv[1])<0||snprintf(stream,sizeof stream,"%s/compact.archive",argv[1])<0)return 2;
 if(mkdir(argv[1],0700)||mkdir(source,0700)||mkdir(raw,0700)||mkdir(compact,0700))return 2;
 Dl_info i;if(!dladdr((void *)(uintptr_t)&mdb_env_copy2,&i))return 2;printf("PROVIDER %s\n",i.dli_fname);printf("CASE variant=%s populated=%d flags=%u\n",variant,populated,flags);
 MDB_env *e=NULL;MDB_txn *t=NULL;MDB_dbi d=0;OK(mdb_env_create(&e));OK(mdb_env_set_maxdbs(e,4));OK(mdb_env_set_mapsize(e,64u<<20));OK(mdb_env_open(e,source,MDB_NOTLS,0600));OK(mdb_txn_begin(e,NULL,0,&t));OK(mdb_dbi_open(t,NULL,flags,&d));
 MDB_val k={1,(void *)"k"},a={sizeof(uint32_t),&vals[0]},b={sizeof(uint32_t),&vals[1]},c={sizeof(uint32_t),&vals[2]};
 if(populated){OK(mdb_put(t,d,&k,&a,0));OK(mdb_put(t,d,&k,&b,0));}OK(mdb_txn_commit(t));
 if(populated){OK(mdb_txn_begin(e,NULL,0,&t));OK(mdb_del(t,d,&k,&b));OK(mdb_put(t,d,&k,&c,0));OK(mdb_txn_commit(t));}
 int rawrc=mdb_env_copy2(e,raw,0),compactrc=mdb_env_copy2(e,compact,MDB_CP_COMPACT);int fd=open(stream,O_RDWR|O_CREAT|O_EXCL,0600);if(fd<0)return 2;int fdrc=mdb_env_copyfd2(e,fd,MDB_CP_COMPACT);close(fd);printf("COPY raw=%d compact=%d compactfd=%d\n",rawrc,compactrc,fdrc);mdb_env_close(e);
 OK(dump(source,"source_current",0));OK(dump(source,"source_previous",1));
 if(!rawrc){OK(dump(raw,"raw_current",0));OK(dump(raw,"raw_previous",1));}
 if(!compactrc){OK(dump(compact,"compact_current",0));OK(dump(compact,"compact_previous",1));}
 if(!rawrc)OK(widths(raw,"raw_after_width",populated));
 if(!compactrc)OK(widths(compact,"compact_after_width",populated));
 printf("CASE_END\n");return 0;
}

