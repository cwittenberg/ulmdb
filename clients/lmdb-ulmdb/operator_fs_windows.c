/* Every destructive operation is rooted in retained handles. No path-based
 * recursive deletion, reparse traversal, replacement or copy/delete publish. */
#include "operator.h"
#include "operator_windows.h"
#include <errno.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>
struct owned_target {
 HANDLE parent,stage;
 FILE_ID_INFO parent_id,stage_id;
 wchar_t *stage_name,*target_name;
 char *path;
 int published;
};
static struct owned_target owner; /* One sequential command owns one stage. */
static int same_id(const FILE_ID_INFO*a,const FILE_ID_INFO*b){return a->VolumeSerialNumber==b->VolumeSerialNumber&&!memcmp(a->FileId.Identifier,b->FileId.Identifier,16);}
static void release_target(void){
 if(owner.stage)CloseHandle(owner.stage);
 if(owner.parent)CloseHandle(owner.parent);
 free(owner.stage_name);free(owner.target_name);free(owner.path);memset(&owner,0,sizeof owner);
}
static int check_stage(void){
 FILE_ID_INFO id;int rc=tool_windows_identity(owner.parent,&id,1);if(rc)return rc;
 if(!same_id(&id,&owner.parent_id))return EIO;
 rc=tool_windows_identity(owner.stage,&id,1);if(rc)return rc;
 if(!same_id(&id,&owner.stage_id))return EIO;
 HANDLE named=NULL;rc=tool_windows_relative(owner.parent,owner.stage_name,1,0,FILE_LIST_DIRECTORY,NULL,&named);
 if(rc)return rc;
 rc=tool_windows_identity(named,&id,1);CloseHandle(named);return rc?rc:same_id(&id,&owner.stage_id)?0:EIO;
}
static int remove_handle(HANDLE h,int directory,unsigned depth){
 if(depth>64)return ELOOP;
 if(directory){
  union {FILE_ID_BOTH_DIR_INFO alignment;unsigned char bytes[65536];} buffer;
  for(;;){
   if(!GetFileInformationByHandleEx(h,FileIdBothDirectoryRestartInfo,buffer.bytes,sizeof buffer.bytes)){
    DWORD e=GetLastError();if(e==ERROR_NO_MORE_FILES)break;return tool_windows_error(e);
   }
   FILE_ID_BOTH_DIR_INFO *entry=(void*)buffer.bytes;
   for(;;){
    size_t n=entry->FileNameLength/sizeof(wchar_t);
    if(entry->FileNameLength%2 || n>32767)return EIO;
    if(!((n==1&&entry->FileName[0]==L'.')||(n==2&&entry->FileName[0]==L'.'&&entry->FileName[1]==L'.'))){
     wchar_t *name=malloc((n+1)*sizeof *name);if(!name)return ENOMEM;
     memcpy(name,entry->FileName,n*sizeof *name);name[n]=0;
     int isdir=(entry->FileAttributes&FILE_ATTRIBUTE_DIRECTORY)!=0;HANDLE child=NULL;
     ACCESS_MASK access=DELETE|FILE_WRITE_ATTRIBUTES|(isdir?(FILE_LIST_DIRECTORY|FILE_TRAVERSE|FILE_DELETE_CHILD):0);
     int rc=tool_windows_relative(h,name,isdir,0,access,NULL,&child);free(name);if(rc)return rc;
     rc=remove_handle(child,isdir,depth+1);CloseHandle(child);if(rc)return rc;
    }
    if(!entry->NextEntryOffset)break;
    size_t at=(unsigned char*)entry-buffer.bytes;
    if(entry->NextEntryOffset>sizeof buffer.bytes-at || entry->NextEntryOffset<offsetof(FILE_ID_BOTH_DIR_INFO,FileName))return EIO;
    entry=(void*)((unsigned char*)entry+entry->NextEntryOffset);
   }
  }
 }
 FILE_BASIC_INFO basic;if(!GetFileInformationByHandleEx(h,FileBasicInfo,&basic,sizeof basic))return tool_windows_error(GetLastError());
 if(basic.FileAttributes&FILE_ATTRIBUTE_READONLY){basic.FileAttributes&=~FILE_ATTRIBUTE_READONLY;if(!SetFileInformationByHandle(h,FileBasicInfo,&basic,sizeof basic))return tool_windows_error(GetLastError());}
 FILE_DISPOSITION_INFO disposition={TRUE};return SetFileInformationByHandle(h,FileDispositionInfo,&disposition,sizeof disposition)?0:tool_windows_error(GetLastError());
}
int tool_prepare_target(const char*target,char**path,int*owned){
 if(owner.parent)return EBUSY;
 wchar_t *given=tool_windows_wide(target);if(!given)return errno?errno:ENOMEM;
 HANDLE existing=CreateFileW(given,FILE_READ_ATTRIBUTES,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,NULL,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,NULL);
 if(existing!=INVALID_HANDLE_VALUE){FILE_ID_INFO id;int rc=tool_windows_identity(existing,&id,1);CloseHandle(existing);free(given);if(rc)return rc;*path=strdup(target);*owned=0;return *path?0:ENOMEM;}
 DWORD error=GetLastError();if(error!=ERROR_FILE_NOT_FOUND&&error!=ERROR_PATH_NOT_FOUND){free(given);return tool_windows_error(error);}
 DWORD size=GetFullPathNameW(given,0,NULL,NULL);if(!size){free(given);return tool_windows_error(GetLastError());}
 wchar_t *full=malloc(((size_t)size+1)*sizeof *full);if(!full){free(given);return ENOMEM;}
 DWORD written=GetFullPathNameW(given,size+1,full,NULL);free(given);if(!written||written>size){free(full);return EINVAL;}
 wchar_t *slash=wcsrchr(full,L'\\');if(!slash||!slash[1]){free(full);return EINVAL;}
 wchar_t *name=_wcsdup(slash+1);if(!name){free(full);return ENOMEM;}
 if(slash==full+2&&full[1]==L':')slash[1]=0;else *slash=0;
 HANDLE parent=CreateFileW(full,FILE_LIST_DIRECTORY|FILE_ADD_FILE|FILE_ADD_SUBDIRECTORY|FILE_TRAVERSE|FILE_READ_ATTRIBUTES|SYNCHRONIZE,
  FILE_SHARE_READ|FILE_SHARE_WRITE,NULL,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,NULL);
 if(parent==INVALID_HANDLE_VALUE){free(name);free(full);return tool_windows_error(GetLastError());}
 FILE_ID_INFO parent_id;int rc=tool_windows_identity(parent,&parent_id,1);if(rc){CloseHandle(parent);free(name);free(full);return rc;}
 PSECURITY_DESCRIPTOR sd=NULL;rc=tool_windows_private_descriptor(&sd);if(rc){CloseHandle(parent);free(name);free(full);return rc;}
 wchar_t stage_name[64];HANDLE stage=NULL;
 for(unsigned attempt=0;attempt<32;attempt++){
  unsigned char random[16];if((rc=tool_windows_random(random,sizeof random)))break;
  static const wchar_t hex[]=L"0123456789abcdef";memcpy(stage_name,L".ulmdb-load-",12*sizeof *stage_name);
  for(size_t i=0;i<sizeof random;i++){stage_name[12+2*i]=hex[random[i]>>4];stage_name[13+2*i]=hex[random[i]&15];}stage_name[44]=0;
  rc=tool_windows_relative(parent,stage_name,1,1,FILE_LIST_DIRECTORY|FILE_ADD_FILE|FILE_ADD_SUBDIRECTORY|FILE_DELETE_CHILD|FILE_TRAVERSE|FILE_WRITE_ATTRIBUTES|DELETE,sd,&stage);
  if(!rc || rc!=EEXIST)break;
 }
 LocalFree(sd);if(rc){CloseHandle(parent);free(name);free(full);return rc;}
 FILE_ID_INFO stage_id;rc=tool_windows_identity(stage,&stage_id,1);
 size_t n=wcslen(full),m=wcslen(stage_name);wchar_t *stage_path=malloc((n+m+2)*sizeof *stage_path);char *utf8=NULL;
 if(stage_path){memcpy(stage_path,full,n*sizeof *full);stage_path[n]=L'\\';memcpy(stage_path+n+1,stage_name,(m+1)*sizeof *stage_name);utf8=tool_windows_utf8(stage_path);}
 free(stage_path);free(full);
 if(rc||!utf8){(void)remove_handle(stage,1,0);CloseHandle(stage);CloseHandle(parent);free(name);free(utf8);return rc?rc:ENOMEM;}
 owner=(struct owned_target){parent,stage,parent_id,stage_id,_wcsdup(stage_name),name,strdup(utf8),0};
 if(!owner.stage_name||!owner.path){(void)remove_handle(stage,1,0);release_target();free(utf8);return ENOMEM;}
 *path=utf8;*owned=1;return 0;
}
void tool_remove_owned(const char*path){
 if(!owner.parent||!owner.path||strcmp(path,owner.path))return;
 if(!owner.published){int rc=check_stage();if(!rc)rc=remove_handle(owner.stage,1,0);if(rc)fprintf(stderr,"ulmdb-tool: owned stage cleanup failed: %d\n",rc);}
 release_target();
}
int tool_publish_target(const char*temporary,const char*target){
 (void)target;/* Target basename is captured before any writes. */
 if(!owner.parent||!owner.path||strcmp(temporary,owner.path))return EINVAL;
 int rc=check_stage();if(rc)return rc;
 if((rc=tool_windows_sync_directory(owner.stage)))return rc;
 size_t n=wcslen(owner.target_name),bytes=offsetof(FILE_RENAME_INFORMATION,FileName)+n*sizeof(wchar_t);if(bytes>ULONG_MAX)return ENAMETOOLONG;
 FILE_RENAME_INFORMATION *rename=calloc(1,bytes);if(!rename)return ENOMEM;
 rename->ReplaceIfExists=FALSE;rename->RootDirectory=owner.parent;rename->FileNameLength=(ULONG)(n*sizeof(wchar_t));memcpy(rename->FileName,owner.target_name,n*sizeof(wchar_t));
 IO_STATUS_BLOCK io={0};NTSTATUS status=NtSetInformationFile(owner.stage,&io,rename,(ULONG)bytes,FileRenameInformation);
 rc=tool_windows_complete(owner.stage,status,&io);free(rename);if(rc)return rc;
 /* Never clean a published target after a failing real directory barrier. */
 owner.published=1;rc=tool_windows_sync_directory(owner.parent);release_target();return rc;
}
