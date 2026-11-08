/* Pinned 1.0 source-observed pure metadata on a DIRTY pending owner.
 * lmdb.h documents only commit/abort after prepare. These probes inventory
 * helper behavior beyond that prose; they grant no general data-operation
 * lifecycle guarantee. Never use clean/readonly/child pointers after prepare.
 * Configure callbacks before any data access, and never change them pending. */
#include <lmdb.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
static unsigned failures, key_calls, dup_calls;
static void ok(int rc,const char *label){if(rc){fprintf(stderr,"%s=%d\n",label,rc);exit(2);}}
static void require(int yes,const char *label){if(!yes){fprintf(stderr,"metadata mismatch: %s\n",label);failures++;}}
static int sign(int value){return (value>0)-(value<0);}
static int reverse_key(const MDB_val*a,const MDB_val*b){key_calls++;size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;int d=memcmp(a->mv_data,b->mv_data,n);return d?-sign(d):((a->mv_size<b->mv_size)-(a->mv_size>b->mv_size));}
static int reverse_dup(const MDB_val*a,const MDB_val*b){dup_calls++;size_t n=a->mv_size<b->mv_size?a->mv_size:b->mv_size;int d=memcmp(a->mv_data,b->mv_data,n);return d?-sign(d):((a->mv_size<b->mv_size)-(a->mv_size>b->mv_size));}
static MDB_val text(const char *value){MDB_val result={strlen(value),(void*)value};return result;}
static void flags(MDB_txn*t,MDB_dbi db,const char *label,unsigned expected){unsigned value=0xfeedu;int rc=mdb_dbi_flags(t,db,&value);printf("%s=%d flags=%u\n",label,rc,value);require(rc==0&&value==expected,label);}
static void compare(MDB_txn*t,MDB_dbi db,const char *label,MDB_val*a,MDB_val*b,int duplicate,int expected){int result=sign(duplicate?mdb_dcmp(t,db,a,b):mdb_cmp(t,db,a,b));printf("%s=%d\n",label,result);require(result==expected,label);}
static const char *names[]={"kv-default","kv-reverse","kv-integer","kv-custom","dup-default","dup-reverse","dup-integer","dup-custom"};
static unsigned mode_flags(unsigned mode){switch(mode){case 1:return MDB_REVERSEKEY;case 2:return MDB_INTEGERKEY;case 4:return MDB_DUPSORT;case 5:return MDB_DUPSORT|MDB_REVERSEDUP;case 6:return MDB_DUPSORT|MDB_DUPFIXED|MDB_INTEGERDUP;case 7:return MDB_DUPSORT;default:return 0;}}
static void scenario(const char *base,unsigned mode,unsigned notls,unsigned commit){char path[4096];int n=snprintf(path,sizeof path,"%s/%u-%u-%u",base,mode,notls,commit);if(n<0||(size_t)n>=sizeof path||mkdir(path,0700)){perror("scenario directory");exit(2);}printf("scenario=prepared-metadata-%s-%s-%s\n",names[mode],notls?"notls":"tls",commit?"commit":"abort");
 MDB_env*env;MDB_txn*t;MDB_dbi main,named;ok(mdb_env_create(&env),"env create");ok(mdb_env_set_maxdbs(env,4),"maxdbs");ok(mdb_env_set_mapsize(env,16u*1024u*1024u),"mapsize");ok(mdb_env_open(env,path,notls?MDB_NOTLS:0,0600),"env open");ok(mdb_txn_begin(env,NULL,0,&t),"write begin");ok(mdb_dbi_open(t,NULL,0,&main),"main open");unsigned expected_flags=mode_flags(mode);ok(mdb_dbi_open(t,"metadata",MDB_CREATE|expected_flags,&named),"named create");
 if(mode==3||mode==7){ok(mdb_set_compare(t,named,reverse_key),"key callback before data");}
 if(mode==7){ok(mdb_set_dupsort(t,named,reverse_dup),"dup callback before data");}
 MDB_val key=text("k"),data=text("dirty");uint32_t number=7;if(mode==2){key.mv_size=sizeof number;key.mv_data=&number;}if(mode==6){data.mv_size=sizeof number;data.mv_data=&number;}ok(mdb_put(t,named,&key,&data,0),"named dirty put");MDB_val main_key=text("m"),main_data=text("dirty");ok(mdb_put(t,main,&main_key,&main_data,0),"main dirty put");
 mdb_size_t before_id=mdb_txn_id(t);key_calls=dup_calls=0;ok(mdb_txn_prepare(t),"dirty top prepare");printf("txn.env.same=%d\n",mdb_txn_env(t)==env);require(mdb_txn_env(t)==env,"txn env");printf("txn.id.same=%d\n",mdb_txn_id(t)==before_id);require(mdb_txn_id(t)==before_id,"txn id");unsigned txn_flags=~0u;int rc=mdb_txn_flags(t,&txn_flags);printf("txn.flags=%d flags=%u\n",rc,txn_flags);require(rc==0&&txn_flags==0,"txn flags");flags(t,main,"main.flags",0);flags(t,named,"named.flags",expected_flags);
 MDB_val a=text("az"),b=text("ba");compare(t,main,"main.cmp.forward",&a,&b,0,-1);compare(t,main,"main.cmp.backward",&b,&a,0,1);uint32_t one=1,two=256;MDB_val ia={sizeof one,&one},ib={sizeof two,&two};MDB_val *ka=mode==2?&ia:&a,*kb=mode==2?&ib:&b;int key_order=(mode==1||mode==3||mode==7)?1:-1;compare(t,named,"named.cmp.forward",ka,kb,0,key_order);compare(t,named,"named.cmp.backward",kb,ka,0,-key_order);
 if(mode>=4){MDB_val *da=mode==6?&ia:&a,*db=mode==6?&ib:&b;int dup_order=(mode==5||mode==7)?1:-1;compare(t,named,"named.dcmp.forward",da,db,1,dup_order);compare(t,named,"named.dcmp.backward",db,da,1,-dup_order);}
 printf("callback.counts=%u,%u\n",key_calls,dup_calls);require(key_calls==((mode==3||mode==7)?2u:0u),"key callbacks");require(dup_calls==(mode==7?2u:0u),"dup callbacks");if(commit){ok(mdb_txn_commit(t),"prepared final commit");puts("finish=commit");}else{mdb_txn_abort(t);puts("finish=abort");}mdb_env_close(env);
}
int main(int argc,char**argv){if(argc!=2)return 2;if(mkdir(argv[1],0700)){perror("fresh root");return 2;}for(unsigned mode=0;mode<8;mode++)for(unsigned notls=0;notls<2;notls++)for(unsigned commit=0;commit<2;commit++)scenario(argv[1],mode,notls,commit);puts("prepare-metadata-complete");if(failures)fprintf(stderr,"metadata mismatches=%u\n",failures);return failures?1:0;}
