/* Windows public-C stream/ownership boundary. No CRT fd is ever passed as a
 * public MDB HANDLE; caller standard handles stay owned by the CRT. */
#ifdef _WIN32
#include "operator_windows.h"
#include <aclapi.h>
#include <sddl.h>
#include <bcrypt.h>
#include <errno.h>
#include <fcntl.h>
#include <io.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
NTSYSAPI NTSTATUS NTAPI NtFlushBuffersFileEx(HANDLE,ULONG,PVOID,ULONG,PIO_STATUS_BLOCK);
int tool_windows_error(DWORD e) {
 switch(e){
 case ERROR_FILE_NOT_FOUND: case ERROR_PATH_NOT_FOUND:return ENOENT;
 case ERROR_FILE_EXISTS:case ERROR_ALREADY_EXISTS:return EEXIST;
 case ERROR_ACCESS_DENIED:case ERROR_PRIVILEGE_NOT_HELD:return EACCES;
 case ERROR_SHARING_VIOLATION:case ERROR_LOCK_VIOLATION:return EBUSY;
 case ERROR_INVALID_HANDLE:return EBADF;
 case ERROR_INVALID_PARAMETER:case ERROR_INVALID_NAME:case ERROR_REPARSE_TAG_INVALID:return EINVAL;
 case ERROR_DIRECTORY:return ENOTDIR;
 case ERROR_DIR_NOT_EMPTY:return ENOTEMPTY;
 case ERROR_FILENAME_EXCED_RANGE:return ENAMETOOLONG;
 case ERROR_NOT_ENOUGH_MEMORY:case ERROR_OUTOFMEMORY:return ENOMEM;
 case ERROR_DISK_FULL:case ERROR_HANDLE_DISK_FULL:return ENOSPC;
 default:fprintf(stderr,"ulmdb-tool: Windows error %lu\n",(unsigned long)e);return EIO;
 }
}
wchar_t *tool_windows_wide(const char *text) {
 int n=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,text,-1,NULL,0);
 if(!n){errno=EINVAL;return NULL;}
 wchar_t *p=malloc((size_t)n*sizeof *p);if(!p)return NULL;
 if(!MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,text,-1,p,n)){free(p);errno=EINVAL;return NULL;}return p;
}
char *tool_windows_utf8(const wchar_t *text) {
 int n=WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,text,-1,NULL,0,NULL,NULL);
 if(!n){errno=EINVAL;return NULL;}char*p=malloc((size_t)n);if(!p)return NULL;
 if(!WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,text,-1,p,n,NULL,NULL)){free(p);errno=EINVAL;return NULL;}return p;
}
int tool_platform_init(void) {
 if(_setmode(_fileno(stdin),_O_BINARY)<0 || _setmode(_fileno(stdout),_O_BINARY)<0)return errno?errno:EIO;
 return 0;
}
int tool_stdio_handle(FILE *file,mdb_filehandle_t *out) {
 intptr_t raw=_get_osfhandle(_fileno(file));
 if(raw==-1 || raw==-2)return EBADF;
 *out=(mdb_filehandle_t)raw;return 0;
}
int tool_windows_random(void *p,size_t size) {
 if(size>ULONG_MAX)return E2BIG;
 return BCryptGenRandom(NULL,p,(ULONG)size,BCRYPT_USE_SYSTEM_PREFERRED_RNG)<0?EIO:0;
}
/* Protected owner-rights DACL: no inherited group/world access, including
 * plaintext spool bytes. OW uses the creating token's actual owner SID. */
int tool_windows_private_descriptor(PSECURITY_DESCRIPTOR *out) {
 return ConvertStringSecurityDescriptorToSecurityDescriptorW(L"D:P(A;OICI;FA;;;OW)",SDDL_REVISION_1,out,NULL)?0:tool_windows_error(GetLastError());
}
int tool_windows_complete(HANDLE h,NTSTATUS result,IO_STATUS_BLOCK *io) {
 if(result==(NTSTATUS)0x103){
  /* Request/name/IOSB must stay live until kernel completion, even on filters. */
  if(WaitForSingleObject(h,INFINITE)!=WAIT_OBJECT_0)abort();
  result=io->Status;
 }
 return result<0?tool_windows_error(RtlNtStatusToDosError(result)):0;
}
int tool_windows_identity(HANDLE h,FILE_ID_INFO *id,int directory) {
 FILE_ATTRIBUTE_TAG_INFO a;
 if(!GetFileInformationByHandleEx(h,FileAttributeTagInfo,&a,sizeof a) ||
    !GetFileInformationByHandleEx(h,FileIdInfo,id,sizeof *id))return tool_windows_error(GetLastError());
 if((a.FileAttributes&FILE_ATTRIBUTE_REPARSE_POINT) || !!(a.FileAttributes&FILE_ATTRIBUTE_DIRECTORY)!=directory)return EINVAL;
 return 0;
}
int tool_windows_relative(HANDLE parent,const wchar_t *name,int directory,int create,ACCESS_MASK access,PSECURITY_DESCRIPTOR sd,HANDLE *out) {
 size_t n=wcslen(name);if(!n||n>32767||wcschr(name,L'/')||wcschr(name,L'\\')||wcschr(name,L':')||!wcscmp(name,L".")||!wcscmp(name,L".."))return EINVAL;
 UNICODE_STRING us={(USHORT)(n*2),(USHORT)(n*2),(PWSTR)name};
 OBJECT_ATTRIBUTES oa;InitializeObjectAttributes(&oa,&us,OBJ_CASE_INSENSITIVE|0x1000,parent,sd);
 IO_STATUS_BLOCK io={0};HANDLE h=NULL;
 NTSTATUS status=NtCreateFile(&h,access|SYNCHRONIZE|FILE_READ_ATTRIBUTES,&oa,&io,NULL,0,
  FILE_SHARE_READ|FILE_SHARE_WRITE|(create?0:FILE_SHARE_DELETE),create?2:1,
  0x200000|0x4000|0x20|(directory?1:0x40),NULL,0);
 int rc=tool_windows_complete(h,status,&io);if(rc){if(h)CloseHandle(h);return rc;}
 FILE_ID_INFO id;rc=tool_windows_identity(h,&id,directory);if(rc){CloseHandle(h);return rc;}*out=h;return 0;
}
int tool_windows_sync_directory(HANDLE h) {
 IO_STATUS_BLOCK io={0};return tool_windows_complete(h,NtFlushBuffersFileEx(h,0,NULL,0,&io),&io);
}
int tool_spool_open(FILE **out) {
 DWORD length=GetTempPathW(0,NULL);if(!length)return tool_windows_error(GetLastError());
 wchar_t *path=malloc(((size_t)length+64)*sizeof *path);if(!path)return ENOMEM;
 if(!GetTempPathW(length,path)){free(path);return tool_windows_error(GetLastError());}
 PSECURITY_DESCRIPTOR sd=NULL;int rc=tool_windows_private_descriptor(&sd);if(rc){free(path);return rc;}
 SECURITY_ATTRIBUTES sa={sizeof sa,sd,FALSE};HANDLE h=INVALID_HANDLE_VALUE;
 for(unsigned attempt=0;attempt<32;attempt++){
  unsigned char random[16];rc=tool_windows_random(random,sizeof random);if(rc)break;
  size_t n=wcslen(path);/* GetTempPath ends in separator. */
  wchar_t *tail=path+n;memcpy(tail,L"ulmdb-tool-field-",17*sizeof *tail);tail+=17;
  static const wchar_t hex[]=L"0123456789abcdef";
  for(size_t i=0;i<sizeof random;i++){tail[2*i]=hex[random[i]>>4];tail[2*i+1]=hex[random[i]&15];}tail[32]=0;
  h=CreateFileW(path,GENERIC_READ|GENERIC_WRITE|DELETE,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,&sa,CREATE_NEW,
    FILE_ATTRIBUTE_TEMPORARY|FILE_FLAG_DELETE_ON_CLOSE|FILE_FLAG_OPEN_REPARSE_POINT,NULL);
  if(h!=INVALID_HANDLE_VALUE)break;
  DWORD e=GetLastError();rc=tool_windows_error(e);if(e!=ERROR_FILE_EXISTS&&e!=ERROR_ALREADY_EXISTS)break;
  path[n]=0;
 }
 LocalFree(sd);free(path);if(h==INVALID_HANDLE_VALUE)return rc?rc:EEXIST;
 int fd=_open_osfhandle((intptr_t)h,_O_RDWR|_O_BINARY|_O_NOINHERIT);
 if(fd<0){rc=errno;CloseHandle(h);return rc;}
 FILE *f=_fdopen(fd,"w+b");if(!f){rc=errno;_close(fd);return rc;}*out=f;return 0;
}
/* Bounded mapped views copy directly into the already-reserved destination.
 * The section/view own kernel refs, and the delete-on-close source stays open.
 * No whole value allocation, file offset truncation, or false adjacent slice. */
int tool_spool_copy(FILE *spool,void *destination,size_t length) {
 if(fflush(spool))return errno?errno:EIO;
 HANDLE h=(HANDLE)_get_osfhandle(_fileno(spool));LARGE_INTEGER bytes;
 if(!GetFileSizeEx(h,&bytes))return tool_windows_error(GetLastError());
 if(bytes.QuadPart<0 || (uint64_t)bytes.QuadPart!=(uint64_t)length)return EIO;
 if(!length)return 0;
 HANDLE section=CreateFileMappingW(h,NULL,PAGE_READONLY,0,0,NULL);
 if(!section)return tool_windows_error(GetLastError());
 SYSTEM_INFO info;GetSystemInfo(&info);size_t offset=0;int rc=0;
 while(offset<length){
  size_t start=offset-(offset%info.dwAllocationGranularity),prefix=offset-start;
  size_t count=length-offset;if(count>16u*1024u*1024u)count=16u*1024u*1024u;
  uint64_t physical=start;void *view=MapViewOfFile(section,FILE_MAP_READ,(DWORD)(physical>>32),(DWORD)physical,prefix+count);
  if(!view){rc=tool_windows_error(GetLastError());break;}
  memcpy((unsigned char*)destination+offset,(unsigned char*)view+prefix,count);
  if(!UnmapViewOfFile(view)){rc=tool_windows_error(GetLastError());break;}offset+=count;
 }
 CloseHandle(section);return rc;
}
#endif
