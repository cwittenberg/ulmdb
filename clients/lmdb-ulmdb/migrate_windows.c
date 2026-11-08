/* Genuine Windows migration: pinned LMDB exporter is a separate process;
 * importer resolves only the explicitly selected adapter DLL. */
#include "migrate_fs.h"
#include "migration_windows.h"
#include <errno.h>
#include <io.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdlib.h>
#include <string.h>
static struct windows_child *input_child;
void migration_windows_input_start(struct windows_child *child){input_child=child;}
ssize_t migration_windows_read(int fd,void *bytes,size_t length){
 HANDLE pipe=(HANDLE)_get_osfhandle(fd);if(pipe==INVALID_HANDLE_VALUE){errno=EBADF;return -1;}
 for(;;){
  DWORD available=0;
  if(!PeekNamedPipe(pipe,NULL,0,NULL,&available,NULL)){DWORD e=GetLastError();if(e==ERROR_BROKEN_PIPE)return 0;errno=tool_windows_error(e);return -1;}
  if(available){DWORD done=0;DWORD n=length>available?available:(DWORD)length;if(!ReadFile(pipe,bytes,n,&done,NULL)){errno=tool_windows_error(GetLastError());return -1;}return done;}
  if(!input_child){errno=EINVAL;return -1;}
  if(GetTickCount64()>=input_child->deadline){errno=ETIMEDOUT;return -1;}
  DWORD wait=WaitForSingleObject(input_child->process,5);if(wait==WAIT_OBJECT_0)return 0;if(wait==WAIT_FAILED){errno=EIO;return -1;}
 }
}
static char *join_utf8(const wchar_t *base,const wchar_t *name){
 size_t a=wcslen(base),b=wcslen(name);wchar_t*p=malloc((a+b+2)*sizeof *p);if(!p)return NULL;
 memcpy(p,base,a*sizeof *p);p[a]=L'\\';memcpy(p+a+1,name,(b+1)*sizeof *p);char*out=tool_windows_utf8(p);free(p);return out;
}
int wmain(int argc,wchar_t **argv){
 const wchar_t *source_arg=NULL,*dest_arg=NULL,*lib_arg=NULL,*export_arg=NULL;int nosubdir=0,quiet=0;char error[1024]={0};
 for(int i=1;i<argc;i++){
  if(!wcscmp(argv[i],L"--quiet"))quiet=1;
  else if(!wcscmp(argv[i],L"--nosubdir"))nosubdir=1;
  else if(!wcscmp(argv[i],L"--library")&&i+1<argc)lib_arg=argv[++i];
  else if(!wcscmp(argv[i],L"--exporter")&&i+1<argc)export_arg=argv[++i];
  else if(!source_arg)source_arg=argv[i];else if(!dest_arg)dest_arg=argv[i];else return 2;
 }
 if(!source_arg||!dest_arg){fputs("usage: ulmdb-migrate SOURCE DESTINATION [--library DLL] [--exporter EXE] [--nosubdir] [--quiet]\n",stderr);return 2;}
 int result=1,single=0,staged=0,published=0,fd=-1,abi_changed=0;struct migration_api api={0};MDB_env*env=NULL;struct migration_info info={0};
 struct windows_child child={0};HANDLE readpipe=NULL,writepipe=NULL,nul=NULL;const char *abi=NULL;
 wchar_t *directory=windows_executable_directory(),*source_w=windows_fullpath(source_arg),*export_w=NULL,*lib_w=NULL;
 char *source=source_w?tool_windows_utf8(source_w):NULL,*library=NULL,*exporter=NULL,*destination=NULL,*stage=NULL,*oldabi=NULL;
 if(!directory||!source){snprintf(error,sizeof error,"resolve source/executable path");goto done;}
 if(migration_source_abi(source,&abi,&single,error,sizeof error))goto done;
 const wchar_t *library_env=_wgetenv(L"ULMDB_LIBRARY");
 if(lib_arg||library_env){lib_w=windows_fullpath(lib_arg?lib_arg:library_env);library=lib_w?tool_windows_utf8(lib_w):NULL;}
 else library=join_utf8(directory,L"ulmdb_c.dll");
 if(export_arg){export_w=windows_fullpath(export_arg);exporter=export_w?tool_windows_utf8(export_w):NULL;}
 else exporter=join_utf8(directory,!strcmp(abi,"0.9")?L"lmdb-export-0.9.exe":L"lmdb-export.exe");
 if(!library||!exporter){snprintf(error,sizeof error,"resolve migration helpers");goto done;}
 if(!export_w)export_w=tool_windows_wide(exporter);
 char *requested=tool_windows_utf8(dest_arg);if(!requested)goto done;
 int rc=migration_destination(requested,&destination,&stage,error,sizeof error);free(requested);if(rc)goto done;staged=1;
 const char *old=getenv("ULMDB_LMDB_ABI");if(old){oldabi=strdup(old);if(!oldabi)goto done;}
 if(_putenv_s("ULMDB_LMDB_ABI",abi)){snprintf(error,sizeof error,"set ABI profile");goto done;}
 abi_changed=1;
 if(migration_load(&api,library,error,sizeof error))goto done;
 SECURITY_ATTRIBUTES sa={sizeof sa,NULL,TRUE};if(!CreatePipe(&readpipe,&writepipe,&sa,0)||!SetHandleInformation(readpipe,HANDLE_FLAG_INHERIT,0)){snprintf(error,sizeof error,"create exporter pipe");goto done;}
 nul=CreateFileW(L"NUL",GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,NULL,OPEN_EXISTING,0,NULL);
 const wchar_t*args[]={export_w,source_w,(nosubdir||single)?L"--nosubdir":NULL,NULL};
 if(windows_child_start(export_w,args,nul,writepipe,&child,error,sizeof error))goto done;
 CloseHandle(writepipe);writepipe=NULL;
 fd=_open_osfhandle((intptr_t)readpipe,_O_RDONLY|_O_BINARY|_O_NOINHERIT);if(fd<0)goto done;readpipe=NULL;migration_windows_input_start(&child);
 if(migration_check(&api,api.env_create(&env),"create destination",error,sizeof error)||
    migration_check(&api,api.env_set_maxdbs(env,65528),"set database capacity",error,sizeof error)||
    migration_read_header(fd,&info,error,sizeof error)||
    migration_check(&api,api.env_set_pagesize(env,info.page_size),"set page geometry",error,sizeof error)||
    migration_check(&api,api.env_set_maxreaders(env,info.max_readers),"set readers",error,sizeof error)||
    migration_check(&api,api.env_set_mapsize(env,(size_t)info.map_size),"set mapsize",error,sizeof error)||
    migration_check(&api,api.env_open(env,stage,MDB_WRITEMAP,0600),"open destination",error,sizeof error)||
    migration_replay(&api,env,fd,&info,error,sizeof error))goto done;
 _close(fd);fd=-1;DWORD status=0;if(windows_child_wait(&child,&status,error,sizeof error)||status){if(status)snprintf(error,sizeof error,"LMDB exporter exited %lu",(unsigned long)status);goto done;}
 api.env_close(env);env=NULL;
 if(migration_metadata(stage,source,abi,&info,error,sizeof error)||migration_publish(stage,destination,&published,error,sizeof error))goto done;
 result=0;if(!quiet)printf("Migrated %"PRIu64" records across %"PRIu64" databases at source transaction %"PRIu64"\n",info.records,info.databases,info.source_sequence);
 done:migration_windows_input_start(NULL);if(env)api.env_close(env);if(fd>=0)_close(fd);if(readpipe)CloseHandle(readpipe);if(writepipe)CloseHandle(writepipe);if(nul&&nul!=INVALID_HANDLE_VALUE)CloseHandle(nul);windows_child_stop(&child);migration_unload(&api);
 if(staged&&!published){char cleanup[512];if(migration_remove_stage(stage,cleanup,sizeof cleanup))fprintf(stderr,"ulmdb-migrate: %s\n",cleanup);}
 if(abi_changed)_putenv_s("ULMDB_LMDB_ABI",oldabi?oldabi:"");
 if(result)fprintf(stderr,"ulmdb-migrate: %s\n",*error?error:"Windows migration failed");
 free(oldabi);free(directory);free(source_w);free(export_w);free(lib_w);free(source);free(library);free(exporter);free(destination);free(stage);return result;
}
