#define _GNU_SOURCE
#include <lmdb.h>
#include <dlfcn.h>
#include <ctype.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
static void need(int yes,const char *label){if(!yes){fprintf(stderr,"oracle: %s\n",label);exit(2);}}
static void ok(int rc,const char *label){if(rc){fprintf(stderr,"call: %s rc=%d\n",label,rc);exit(2);}}
static void advice(const MDB_val *v,int random){
    FILE*f=fopen("/proc/self/smaps","r");need(f!=NULL,"smaps open");char line[8192],header[8192]="";int active=0;
    while(fgets(line,sizeof line,f)){unsigned long a,b;if(sscanf(line,"%lx-%lx",&a,&b)==2){active=a<=(uintptr_t)v->mv_data&&(uintptr_t)v->mv_data<b;if(active)strcpy(header,line);}
        if(active&&!strncmp(line,"VmFlags:",8)){int rr=0; char *word=line+8;
            while(*word){while(isspace((unsigned char)*word))word++;char *end=word;while(*end&&!isspace((unsigned char)*end))end++;if(end-word==2&&word[0]=='r'&&word[1]=='r')rr=1;word=end;}need(strstr(header,".seg")||strstr(header,".cohort")||strstr(header,"data.mdb"),"genuine file mapped value");need(rr==random,"real kernel random advice");fclose(f);return;}}
    fclose(f);need(0,"borrowed mapping not found");
}
int main(int argc,char**argv){
    need(argc==3,"usage path logical|kernel");int kernel=!strcmp(argv[2],"kernel");need(kernel||!strcmp(argv[2],"logical"),"mode");
    if(mkdir(argv[1],0700)&&errno!=EEXIST){perror("mkdir");return 2;}
    Dl_info d;need(dladdr((void*)mdb_env_open,&d)&&d.dli_fname,"provider");fprintf(stderr,"PROVIDER %s\n",d.dli_fname);
    for(int phase=0;phase<3;phase++){
        unsigned option=phase==1?MDB_NORDAHEAD:0,flags=0;MDB_env*env=NULL;MDB_txn*w=NULL,*r=NULL;MDB_dbi db;MDB_val k={3,"key"},v;
        unsigned char data[8192];memset(data,'a'+2*phase,sizeof data);
        ok(mdb_env_create(&env),"create");need(mdb_env_set_flags(env,MDB_NORDAHEAD,1)==EINVAL,"preopen set on");need(mdb_env_set_flags(env,MDB_NORDAHEAD,0)==EINVAL,"preopen set off");
        ok(mdb_env_set_mapsize(env,64u*1024u*1024u),"mapsize");ok(mdb_env_open(env,argv[1],MDB_NOTLS|option,0600),"open");
        ok(mdb_env_get_flags(env,&flags),"flags");need((flags&MDB_NORDAHEAD)==option,"open bit");need(mdb_env_set_flags(env,MDB_NORDAHEAD,0)==EINVAL,"opened set off");need(mdb_env_set_flags(env,MDB_NORDAHEAD,1)==EINVAL,"opened set on");
        ok(mdb_txn_begin(env,NULL,0,&w),"first begin");ok(mdb_dbi_open(w,NULL,0,&db),"main open");
        if(phase){ok(mdb_get(w,db,&k,&v),"reopen get");need(v.mv_size==8192&&((unsigned char*)v.mv_data)[0]=='a'+2*phase-1,"reopen bytes");if(kernel){advice(&v,option!=0);}}
        v.mv_size=sizeof data;v.mv_data=data;ok(mdb_put(w,db,&k,&v,0),"seed put");size_t first=mdb_txn_id(w);need(first==(size_t)(2*phase+1),"absolute first ID");ok(mdb_txn_commit(w),"seed commit");w=NULL;
        ok(mdb_txn_begin(env,NULL,MDB_RDONLY,&r),"old read");ok(mdb_get(r,db,&k,&v),"old get");need(v.mv_size==8192&&!memcmp(v.mv_data,data,sizeof data),"old bytes");if(kernel){advice(&v,option!=0);}
        MDB_val borrowed=v;ok(mdb_txn_begin(env,NULL,0,&w),"update begin");memset(data,'b'+2*phase,sizeof data);v.mv_data=data;v.mv_size=sizeof data;ok(mdb_put(w,db,&k,&v,0),"update put");size_t second=mdb_txn_id(w);need(second==first+1,"absolute second ID");ok(mdb_txn_commit(w),"update commit");w=NULL;
        for(size_t i=0;i<borrowed.mv_size;i++){need(((unsigned char*)borrowed.mv_data)[i]=='a'+2*phase,"old borrow after commit");}
        if(kernel){advice(&borrowed,option!=0);}mdb_txn_abort(r);r=NULL;
        ok(mdb_txn_begin(env,NULL,MDB_RDONLY,&r),"fresh read");ok(mdb_get(r,db,&k,&v),"fresh get");need(v.mv_size==8192&&!memcmp(v.mv_data,data,sizeof data),"fresh bytes");if(kernel){advice(&v,option!=0);}ok(mdb_txn_commit(r),"read commit");r=NULL;
        MDB_envinfo info;ok(mdb_env_info(env,&info),"info");need(info.me_last_txnid==second,"visible ID");printf("phase=%d nordahead=%u first=%zu second=%zu bytes=8192 old=%c new=%c\n",phase,!!option,first,second,'a'+2*phase,'b'+2*phase);mdb_env_close(env);
    }
    puts("noreadahead-contract=ok");return 0;
}
