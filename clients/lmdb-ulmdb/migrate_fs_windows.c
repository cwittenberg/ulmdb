/* Windows importer staging reuses the operator's held root/NT no-replace owner. */
#include "migrate_fs.h"
#include "operator.h"
#include "operator_windows.h"
#include <errno.h>
#include <fcntl.h>
#include <io.h>
#include <inttypes.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
int migration_error(char*out,size_t n,const char*format,...){va_list args;va_start(args,format);vsnprintf(out,n,format,args);va_end(args);return -1;}
char *migration_join(const char*parent,const char*name){size_t a=strlen(parent),b=strlen(name);if(a>SIZE_MAX-b-2){errno=ENOMEM;return NULL;}char*p=malloc(a+b+2);if(p)snprintf(p,a+b+2,"%s/%s",parent,name);return p;}
static uint32_t word(const unsigned char*p,int big){uint32_t x=0;for(unsigned i=0;i<4;i++)x|=(uint32_t)p[big?3-i:i]<<(8*i);return x;}
int migration_source_abi(const char*source,const char**abi,int*single,char*error,size_t n){
 wchar_t*w=tool_windows_wide(source);if(!w)return migration_error(error,n,"invalid source path");
 HANDLE h=CreateFileW(w,FILE_READ_ATTRIBUTES,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,NULL,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,NULL);free(w);
 if(h==INVALID_HANDLE_VALUE)return migration_error(error,n,"inspect LMDB source: Windows error %lu",(unsigned long)GetLastError());
 FILE_ATTRIBUTE_TAG_INFO tag;int rc=GetFileInformationByHandleEx(h,FileAttributeTagInfo,&tag,sizeof tag)?0:tool_windows_error(GetLastError());CloseHandle(h);
 if(rc||tag.FileAttributes&FILE_ATTRIBUTE_REPARSE_POINT)return migration_error(error,n,"source is unavailable or a reparse point");
 *single=!(tag.FileAttributes&FILE_ATTRIBUTE_DIRECTORY);
 char*data=*single?strdup(source):migration_join(source,"data.mdb");if(!data)return migration_error(error,n,"allocate source path");
 w=tool_windows_wide(data);free(data);if(!w)return migration_error(error,n,"source path conversion");
 h=CreateFileW(w,GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,NULL,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,NULL);free(w);
 if(h==INVALID_HANDLE_VALUE)return migration_error(error,n,"open LMDB data: Windows error %lu",(unsigned long)GetLastError());
 FILE_ID_INFO id;rc=tool_windows_identity(h,&id,0);unsigned char bytes[64];DWORD got=0;
 if(!rc&&!ReadFile(h,bytes,sizeof bytes,&got,NULL))rc=tool_windows_error(GetLastError());
 CloseHandle(h);
 if(rc)return migration_error(error,n,"read regular LMDB data header: %s",strerror(rc));
 static const size_t positions[]={24,20,16,12};
 for(size_t i=0;i<sizeof positions/sizeof *positions;i++)if(got>=positions[i]+8)for(int big=0;big<=1;big++)if(word(bytes+positions[i],big)==UINT32_C(0xbeefc0de)){
  uint32_t v=word(bytes+positions[i]+4,big);if(v==1||v==3){*abi=v==1?"0.9":"1.0";return 0;}return migration_error(error,n,"unsupported LMDB format %u",v);
 }return migration_error(error,n,"invalid LMDB metadata");
}
int migration_destination(const char*requested,char**destination,char**stage,char*error,size_t n){
 int owned=0,rc=tool_prepare_target(requested,stage,&owned);
 if(rc)return migration_error(error,n,"create owned destination stage: %s",strerror(rc));
 if(!owned){free(*stage);*stage=NULL;return migration_error(error,n,"Migration destination already exists");}
 *destination=strdup(requested);if(!*destination){tool_remove_owned(*stage);free(*stage);*stage=NULL;return migration_error(error,n,"allocate destination");}return 0;
}
static FILE*metadata_file(const char*stage,const char*name,char*error,size_t n){
 char*path=migration_join(stage,name);if(!path)return NULL;wchar_t*w=tool_windows_wide(path);free(path);if(!w)return NULL;
 PSECURITY_DESCRIPTOR sd=NULL;int rc=tool_windows_private_descriptor(&sd);if(rc){free(w);return NULL;}
 SECURITY_ATTRIBUTES sa={sizeof sa,sd,FALSE};HANDLE h=CreateFileW(w,GENERIC_WRITE,FILE_SHARE_READ,&sa,CREATE_NEW,FILE_ATTRIBUTE_NORMAL|FILE_FLAG_OPEN_REPARSE_POINT,NULL);
 LocalFree(sd);free(w);
 if(h==INVALID_HANDLE_VALUE){migration_error(error,n,"create provenance: Windows error %lu",(unsigned long)GetLastError());return NULL;}
 int fd=_open_osfhandle((intptr_t)h,_O_WRONLY|_O_BINARY|_O_NOINHERIT);if(fd<0){CloseHandle(h);return NULL;}
 FILE*f=_fdopen(fd,"wb");if(!f)_close(fd);return f;
}
static void json_path(FILE*f,const char*s){fputc('"',f);for(const unsigned char*p=(const unsigned char*)s;*p;p++){if(*p=='"'||*p=='\\')fputc('\\',f);if(*p<32)fprintf(f,"\\u%04x",*p);else fputc(*p,f);}fputc('"',f);}
int migration_metadata(const char*stage,const char*source,const char*abi,const struct migration_info*info,char*error,size_t n){
 char*reader=migration_join(stage,"LMDB-READERS");wchar_t*w=reader?tool_windows_wide(reader):NULL;free(reader);if(!w)return migration_error(error,n,"allocate readers path");
 if(!DeleteFileW(w)&&GetLastError()!=ERROR_FILE_NOT_FOUND){DWORD e=GetLastError();free(w);return migration_error(error,n,"remove private reader config: Windows error %lu",(unsigned long)e);}free(w);
 FILE*f=metadata_file(stage,"LMDB_MIGRATION.json",error,n);if(!f)return -1;
 fputs("{\n  \"schema\":1,\n  \"source\":",f);json_path(f,source);
 fprintf(f,",\n  \"source_sequence\":%"PRIu64",\n  \"max_readers\":%u,\n  \"map_size\":%"PRIu64",\n  \"databases\":%"PRIu64",\n  \"records\":%"PRIu64",\n  \"abi\":\"%s\",\n  \"page_size\":%u\n}\n",info->source_sequence,info->max_readers,info->map_size,info->databases,info->records,abi,info->page_size);
 int rc=ferror(f)||fflush(f)||_commit(_fileno(f));if(fclose(f))rc=1;return rc?migration_error(error,n,"flush migration provenance"):0;
}
int migration_publish(const char*stage,const char*destination,int*published,char*error,size_t n){int rc=tool_publish_target(stage,destination);if(!rc)*published=1;return rc?migration_error(error,n,"publish destination: %s",strerror(rc)):0;}
int migration_remove_stage(const char*stage,char*error,size_t n){(void)error;(void)n;tool_remove_owned(stage);return 0;}
