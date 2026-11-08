/* Compile-only on non-Windows CI; run on Windows with a NEW missing target.
 * Exercises actual private spool/mapping and rooted target publication. */
#include "operator.h"
#include "operator_windows.h"
#include <stdlib.h>
#include <string.h>
int wmain(int argc,wchar_t **argv){
 if(argc!=2)return 2;
 int rc=tool_platform_init();if(rc)return 3;
 char *target=tool_windows_utf8(argv[1]),*path=NULL;int owned=0;FILE *spool=NULL;
 unsigned char block[65536],*value=NULL;size_t size=3u*1024u*1024u+7;
 if(!target)return 4;
 if((rc=tool_prepare_target(target,&path,&owned))||!owned){free(target);return 5;}
 if((rc=tool_spool_open(&spool)))goto done;
 for(size_t i=0;i<sizeof block;i++)block[i]=(unsigned char)(i%251);
 for(size_t n=0;n<size;){size_t part=size-n;if(part>sizeof block)part=sizeof block;if(fwrite(block,1,part,spool)!=part){rc=6;goto done;}n+=part;}
 value=malloc(size);if(!value){rc=7;goto done;}
 if((rc=tool_spool_copy(spool,value,size)))goto done;
 for(size_t n=0;n<size;n++)if(value[n]!=block[n%sizeof block]){rc=8;goto done;}
 rc=tool_publish_target(path,target);if(!rc)owned=0;
 done:free(value);if(spool)fclose(spool);if(owned)tool_remove_owned(path);free(path);free(target);return rc?1:0;
}
