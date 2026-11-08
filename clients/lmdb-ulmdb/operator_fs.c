#ifdef _WIN32
#include "operator_fs_windows.c"
#elif defined(__APPLE__)
#include "operator_fs_darwin.c"
#else
/* Only the command's own newly-created staging tree is removed on failure. */
#include "operator.h"
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/syscall.h>
#include <unistd.h>
#include <dirent.h>
#include <fcntl.h>
int tool_prepare_target(const char*target,char**path,int*owned){
 struct stat s;if(!lstat(target,&s)){if(!S_ISDIR(s.st_mode))return ENOTDIR;*path=strdup(target);*owned=0;return *path?0:ENOMEM;}if(errno!=ENOENT)return errno;
 size_t n=strlen(target);char*tmp=malloc(n+20);if(!tmp)return ENOMEM;memcpy(tmp,target,n);memcpy(tmp+n,".load-XXXXXX",13);if(!mkdtemp(tmp)){int rc=errno;free(tmp);return rc;}*path=tmp;*owned=1;return 0;
}
void tool_remove_owned(const char*path){
 DIR*d=opendir(path);if(!d)return;struct dirent*entry;
 while((entry=readdir(d))){if(!strcmp(entry->d_name,".")||!strcmp(entry->d_name,".."))continue;size_t n=strlen(path)+strlen(entry->d_name)+2;char*p=malloc(n);if(!p)continue;snprintf(p,n,"%s/%s",path,entry->d_name);struct stat s;if(!lstat(p,&s)){if(S_ISDIR(s.st_mode))tool_remove_owned(p);else(void)unlink(p);}free(p);}
 closedir(d);(void)rmdir(path);
}
int tool_publish_target(const char*temporary,const char*target){
#ifdef SYS_renameat2
 if(syscall(SYS_renameat2,AT_FDCWD,temporary,AT_FDCWD,target,1))return errno;
#else
 (void)temporary;(void)target;return ENOTSUP;
#endif
 char*parent=strdup(target);if(!parent)return ENOMEM;char*slash=strrchr(parent,'/');if(slash){if(slash==parent)slash[1]=0;else *slash=0;}else{free(parent);parent=strdup(".");if(!parent)return ENOMEM;}
 int fd=open(parent,O_RDONLY|O_DIRECTORY|O_CLOEXEC);free(parent);if(fd<0)return errno;int rc=fsync(fd)?errno:0;close(fd);return rc;
}

#endif
