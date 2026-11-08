/* Normal Windows launcher: inherited DLL directory + PATH, no debugger,
 * injection or application-directory edits. Private/bundled bindings remain
 * outside the dynamic replacement contract. */
#include "windows_process.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static wchar_t *append(const wchar_t*a,const wchar_t*b){size_t x=wcslen(a),y=wcslen(b);if(x>SIZE_MAX-y-1)return NULL;wchar_t*p=malloc((x+y+1)*sizeof *p);if(p){memcpy(p,a,x*sizeof *p);memcpy(p+x,b,(y+1)*sizeof *p);}return p;}
static int id_at(const wchar_t*path,FILE_ID_INFO*id){
 HANDLE h=CreateFileW(path,FILE_READ_ATTRIBUTES,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,NULL,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,NULL);
 if(h==INVALID_HANDLE_VALUE)return GetLastError()==ERROR_FILE_NOT_FOUND?0:-1;
 int rc=tool_windows_identity(h,id,0);CloseHandle(h);return rc?-1:1;
}
static int bundled_conflict(const wchar_t*application,const wchar_t*package){
 wchar_t*directory=_wcsdup(application);if(!directory)return 1;wchar_t*slash=wcsrchr(directory,L'\\');if(!slash){free(directory);return 1;}*slash=0;
 const wchar_t*names[]={L"\\lmdb.dll",L"\\liblmdb.dll",L"\\ulmdb_c.dll",L"\\libulmdb_c.dll",NULL};
 for(size_t i=0;names[i];i++){
  wchar_t*local=append(directory,names[i]),*expected=append(package,names[i]);FILE_ID_INFO a,b;
  if(!local||!expected){free(local);free(expected);free(directory);return 1;}
  int exists=id_at(local,&a),normal=id_at(expected,&b);
  free(local);free(expected);
  if(exists<0||(exists&&(!normal||normal<0||a.VolumeSerialNumber!=b.VolumeSerialNumber||memcmp(a.FileId.Identifier,b.FileId.Identifier,16)))){
   fwprintf(stderr,L"convert-lmdb-to-ulmdb: application has private/bundled %ls; replace that binding explicitly or use an application without a private LMDB DLL\n",names[i]+1);free(directory);return 1;
  }
 }free(directory);return 0;
}
int wmain(int count,wchar_t **arguments){
 int at=1;const wchar_t*abi=NULL;
 if(count>1&&!wcscmp(arguments[1],L"--abi")){
  if(count<4||(wcscmp(arguments[2],L"0.9.31")&&wcscmp(arguments[2],L"0.9.36")&&wcscmp(arguments[2],L"1.0.2")))return 2;
  abi=arguments[2];at=3;
 }
 if(count<=at){fputs("usage: convert-lmdb-to-ulmdb [--abi 0.9.31|0.9.36|1.0.2] APPLICATION [ARGUMENT ...]\n",stderr);return 2;}
 wchar_t *binary=windows_executable_directory(),*application=NULL,*search=NULL,*migrator=NULL,*library=NULL;int rc=1;char error[512]={0};struct windows_child child={0};
 DWORD n=SearchPathW(NULL,arguments[at],L".exe",0,NULL,NULL);
 if(n){application=malloc(((size_t)n+1)*sizeof *application);if(application&&!SearchPathW(NULL,arguments[at],L".exe",n+1,application,NULL)){free(application);application=NULL;}}
 if(!binary||!application){fputs("convert-lmdb-to-ulmdb: cannot resolve application/package\n",stderr);goto done;}
 if(bundled_conflict(application,binary))goto done;
 const wchar_t*prior=_wgetenv(L"PATH");wchar_t*prefix=append(binary,L";");search=prefix?append(prefix,prior?prior:L""):NULL;free(prefix);
 migrator=append(binary,L"\\ulmdb-migrate.exe");library=append(binary,L"\\ulmdb_c.dll");
 if(!search||!migrator||!library||_wputenv_s(L"PATH",search)||(abi&&_wputenv_s(L"ULMDB_LMDB_ABI",abi))||
    (!_wgetenv(L"ULMDB_MIGRATOR")&&_wputenv_s(L"ULMDB_MIGRATOR",migrator))||
    (!_wgetenv(L"ULMDB_LIBRARY")&&_wputenv_s(L"ULMDB_LIBRARY",library))||!SetDllDirectoryW(binary)){
  fputs("convert-lmdb-to-ulmdb: cannot configure package search path\n",stderr);goto done;
 }
 if(windows_application_start(application,(const wchar_t*const*)(arguments+at),&child,error,sizeof error)){fprintf(stderr,"convert-lmdb-to-ulmdb: %s\n",error);goto done;}
 if(WaitForSingleObject(child.process,INFINITE)!=WAIT_OBJECT_0){fputs("convert-lmdb-to-ulmdb: child wait failed\n",stderr);goto done;}
 DWORD status;if(!GetExitCodeProcess(child.process,&status))goto done;rc=(int)status;
 done:windows_child_stop(&child);free(binary);free(application);free(search);free(migrator);free(library);return rc;
}
