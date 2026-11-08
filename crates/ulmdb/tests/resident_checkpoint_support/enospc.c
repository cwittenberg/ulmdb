#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <unistd.h>
#include <sys/types.h>
static int fired;
static int fail(int fd,const char *kind){
 const char *mode=getenv("RESIDENT_CHECKPOINT_ENOSPC");if(!mode||fired||strcmp(mode,kind))return 0;
 char link[64],path[4096];snprintf(link,sizeof(link),"/proc/self/fd/%d",fd);ssize_t n=readlink(link,path,sizeof(path)-1);if(n<0)return 0;path[n]=0;
 if(!strstr(path,!strcmp(kind,"image")?"/.resident-checkpoint-":"/.resident-wal-"))return 0;
 fired=1;const char marker[]="RESIDENT_CHECKPOINT_REAL_ENOSPC\n";ssize_t(*real)(int,const void*,size_t)=dlsym(RTLD_NEXT,"write");real(2,marker,sizeof(marker)-1);errno=ENOSPC;return 1;
}
ssize_t write(int fd,const void *p,size_t n){ssize_t(*real)(int,const void*,size_t)=dlsym(RTLD_NEXT,"write");if(fail(fd,"image"))return -1;return real(fd,p,n);}
ssize_t pwrite64(int fd,const void *p,size_t n,off64_t at){ssize_t(*real)(int,const void*,size_t,off64_t)=dlsym(RTLD_NEXT,"pwrite64");if(fail(fd,"wal"))return -1;return real(fd,p,n,at);}
