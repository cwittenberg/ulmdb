/* Adapter-only ownership control: upstream forbids inherited environment use.
 * The child must reject before NOLOCK fresh authority or NOSYNC skip; the
 * creator retains its writer and can still force-sync selected state. */
#include <lmdb.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>
static void ok(int rc,const char*label){if(rc){fprintf(stderr,"%s=%d\n",label,rc);exit(2);}}
int main(int argc,char**argv){
 if(argc!=5)return 2;
 setvbuf(stdout,NULL,_IONBF,0);
 int nolock=atoi(argv[2]),nosync=atoi(argv[3]),force=atoi(argv[4]);
 if(mkdir(argv[1],0700))return 2;
 MDB_env*env;ok(mdb_env_create(&env),"create");ok(mdb_env_set_mapsize(env,4u*1024u*1024u),"mapsize");
 unsigned flags=MDB_NOTLS|(nolock?MDB_NOLOCK:0)|(nosync?MDB_NOSYNC:0);
 ok(mdb_env_open(env,argv[1],flags,0600),"open");
 pid_t child=fork();if(child<0)return 2;
 if(!child){int rc=mdb_env_sync(env,force);printf("child.sync=%d expected=%d\n",rc,EINVAL);_exit(rc==EINVAL?0:1);}
 int status;if(waitpid(child,&status,0)!=child)return 2;
 int rejected=WIFEXITED(status)&&WEXITSTATUS(status)==0;
 printf("child.rejected=%d\n",rejected);
 MDB_txn*txn=NULL;int rc=mdb_txn_begin(env,NULL,0,&txn);printf("parent.writer.begin=%d\n",rc);ok(rc,"creator writer");
 rc=mdb_env_sync(env,1);printf("parent.retained_writer.sync.force1=%d\n",rc);ok(rc,"creator retained writer force sync");
 mdb_txn_abort(txn);mdb_env_close(env);puts("creator.progress=ok");
 return rejected?0:1;
}
