/* Explicit argument quoting and a kill-on-close job own every created child.
 * Only three duplicated standard handles enter the child handle list. */
#ifdef _WIN32
#include "windows_process.h"
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
wchar_t *windows_fullpath(const wchar_t *input){
 DWORD n=GetFullPathNameW(input,0,NULL,NULL);if(!n)return NULL;
 wchar_t *p=malloc(((size_t)n+1)*sizeof *p);if(!p)return NULL;
 DWORD got=GetFullPathNameW(input,n+1,p,NULL);if(!got||got>n){free(p);return NULL;}return p;
}
wchar_t *windows_executable_directory(void){
 for(DWORD size=256;size<=32768;size*=2){
  wchar_t *p=malloc(((size_t)size+1)*sizeof *p);if(!p)return NULL;
  DWORD n=GetModuleFileNameW(NULL,p,size);if(n&&n<size){p[n]=0;wchar_t *slash=wcsrchr(p,L'\\');if(!slash){free(p);return NULL;}*slash=0;return p;}free(p);
 }return NULL;
}
static wchar_t *command_line(const wchar_t *const *args){
 size_t n=1;for(size_t i=0;args[i];i++){size_t len=wcslen(args[i]);if(len>(SIZE_MAX-n-4)/2)return NULL;n+=2*len+4;}
 if(n>32767)return NULL;
 wchar_t *line=malloc(n*sizeof *line);if(!line)return NULL;wchar_t *p=line;
 for(size_t i=0;args[i];i++){
  if(i)*p++=L' ';
  *p++=L'"';size_t slash=0;
  for(const wchar_t*s=args[i];;s++){
   if(*s==L'\\'){slash++;continue;}
   if(*s==L'"'||!*s){for(size_t k=0;k<2*slash;k++)*p++=L'\\';if(*s)*p++=L'\\';}
   else for(size_t k=0;k<slash;k++)*p++=L'\\';
   slash=0;if(!*s)break;*p++=*s;
  }*p++=L'"';
 }*p=0;return line;
}
static HANDLE inherited(HANDLE source,DWORD access){
 HANDLE output=NULL;
 if(source&&source!=INVALID_HANDLE_VALUE&&DuplicateHandle(GetCurrentProcess(),source,GetCurrentProcess(),&output,0,TRUE,DUPLICATE_SAME_ACCESS))return output;
 SECURITY_ATTRIBUTES sa={sizeof sa,NULL,TRUE};return CreateFileW(L"NUL",access,FILE_SHARE_READ|FILE_SHARE_WRITE,&sa,OPEN_EXISTING,0,NULL);
}
void windows_child_stop(struct windows_child *c){
 if(c->job)TerminateJobObject(c->job,1);
 if(c->process){WaitForSingleObject(c->process,10000);CloseHandle(c->process);}
 if(c->job)CloseHandle(c->job);
 memset(c,0,sizeof *c);
}
static int start(const wchar_t *executable,const wchar_t *const *args,HANDLE input,HANDLE output,struct windows_child *child,char *error,size_t size,int own_tree){
 HANDLE stdio[3]={inherited(input?input:GetStdHandle(STD_INPUT_HANDLE),GENERIC_READ),inherited(output?output:GetStdHandle(STD_OUTPUT_HANDLE),GENERIC_WRITE),inherited(GetStdHandle(STD_ERROR_HANDLE),GENERIC_WRITE)};
 LPPROC_THREAD_ATTRIBUTE_LIST attrs=NULL;wchar_t *line=NULL;int result=-1;PROCESS_INFORMATION process={0};HANDLE job=NULL;
 for(size_t i=0;i<3;i++)if(!stdio[i]||stdio[i]==INVALID_HANDLE_VALUE)goto done;
 SIZE_T bytes=0;InitializeProcThreadAttributeList(NULL,1,0,&bytes);attrs=malloc(bytes);if(!attrs)goto done;
 if(!InitializeProcThreadAttributeList(attrs,1,0,&bytes)){free(attrs);attrs=NULL;goto done;}
 if(!UpdateProcThreadAttribute(attrs,0,PROC_THREAD_ATTRIBUTE_HANDLE_LIST,stdio,sizeof stdio,NULL,NULL))goto done;
 line=command_line(args);if(!line){SetLastError(ERROR_BAD_LENGTH);goto done;}
 if(own_tree){
  job=CreateJobObjectW(NULL,NULL);if(!job)goto done;
  JOBOBJECT_EXTENDED_LIMIT_INFORMATION limit={0};limit.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
  if(!SetInformationJobObject(job,JobObjectExtendedLimitInformation,&limit,sizeof limit))goto done;
 }
 STARTUPINFOEXW startup;memset(&startup,0,sizeof startup);startup.StartupInfo.cb=sizeof startup;startup.StartupInfo.dwFlags=STARTF_USESTDHANDLES;
 startup.StartupInfo.hStdInput=stdio[0];startup.StartupInfo.hStdOutput=stdio[1];startup.StartupInfo.hStdError=stdio[2];startup.lpAttributeList=attrs;
 if(!CreateProcessW(executable,line,NULL,NULL,TRUE,EXTENDED_STARTUPINFO_PRESENT|CREATE_SUSPENDED|CREATE_UNICODE_ENVIRONMENT,NULL,NULL,&startup.StartupInfo,&process))goto done;
 if((job&&!AssignProcessToJobObject(job,process.hProcess))||ResumeThread(process.hThread)==(DWORD)-1){TerminateProcess(process.hProcess,1);WaitForSingleObject(process.hProcess,10000);goto done;}
 *child=(struct windows_child){process.hProcess,job,process.dwProcessId,GetTickCount64()+120000};process.hProcess=NULL;job=NULL;result=0;
 done:if(result)snprintf(error,size,"create owned Windows child: error %lu",(unsigned long)GetLastError());
 if(process.hThread)CloseHandle(process.hThread);
 if(process.hProcess)CloseHandle(process.hProcess);
 if(job)CloseHandle(job);
 if(attrs){DeleteProcThreadAttributeList(attrs);free(attrs);}free(line);
 for(size_t i=0;i<3;i++)if(stdio[i]&&stdio[i]!=INVALID_HANDLE_VALUE)CloseHandle(stdio[i]);
 return result;
}
int windows_child_start(const wchar_t *exe,const wchar_t *const *args,HANDLE input,HANDLE output,struct windows_child *child,char *error,size_t size){return start(exe,args,input,output,child,error,size,1);}
/* Arbitrary applications retain ordinary process/descendant lifetime. No Job
 * assignment, termination on wrapper close, debugger or console-group change. */
int windows_application_start(const wchar_t *exe,const wchar_t *const *args,struct windows_child *child,char *error,size_t size){return start(exe,args,NULL,NULL,child,error,size,0);}
int windows_child_wait(struct windows_child *c,DWORD *code,char *error,size_t size){
 ULONGLONG now=GetTickCount64();DWORD left=now>=c->deadline?0:(DWORD)(c->deadline-now);
 if(WaitForSingleObject(c->process,left)!=WAIT_OBJECT_0){snprintf(error,size,"Windows child exceeded120second deadline or wait failed");return -1;}
 if(!GetExitCodeProcess(c->process,code)){snprintf(error,size,"Windows child exit status error %lu",(unsigned long)GetLastError());return -1;}return 0;
}
#endif
