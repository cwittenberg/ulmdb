/* Streaming records; large input fields use an unlinked private spool.
 * VERSION=3 bytevalue interoperability follows upstream LMDB mdb_dump/load.
 * ULMDB-DUMP=2 adds an explicit MAIN/NAMED kind, including named empty.
 * The version1 reader retains its original empty-name-means-MAIN grammar. */
#include "operator.h"
#include <errno.h>
#include <stdint.h>
#include <fcntl.h>
#ifdef _WIN32
#include "operator_windows.h"
#define getc_unlocked _getc_nolock
#else
#include <unistd.h>
#endif
#include <stdlib.h>
#include <string.h>
static size_t line_number;
struct header{char*name;unsigned flags;size_t pagesize,mapsize;};
static const struct{unsigned bit;const char*name;} flags[]={{MDB_REVERSEKEY,"reversekey"},{MDB_DUPSORT,"dupsort"},{MDB_INTEGERKEY,"integerkey"},{MDB_DUPFIXED,"dupfixed"},{MDB_INTEGERDUP,"integerdup"},{MDB_REVERSEDUP,"reversedup"}};
static int bad(const char*why){fprintf(stderr,"ulmdb-tool load: line %zu: %s\n",line_number,why);return EINVAL;}
static int nibble(int c){if(c>='0'&&c<='9')return c-'0';if(c>='a'&&c<='f')return c-'a'+10;if(c>='A'&&c<='F')return c-'A'+10;return -1;}
int tool_hex(FILE *f, const void *bytes, size_t n) {
 static const char hex[] = "0123456789abcdef";
 const unsigned char *p = bytes;
 char output[TOOL_IO_BYTES];
 while (n) {
  size_t count = n < sizeof output / 2 ? n : sizeof output / 2;
  for (size_t i = 0; i < count; ++i) {
   output[2*i] = hex[p[i] >> 4];
   output[2*i+1] = hex[p[i] & 15];
  }
  if (fwrite(output, 2, count, f) != count) return errno ? errno : EIO;
  p += count;
  n -= count;
 }
 return 0;
}
static int reserve(struct tool_buffer*b,size_t n,size_t limit){if(n>limit)return E2BIG;if(n<=b->capacity)return 0;size_t cap=b->capacity?b->capacity:64;while(cap<n){if(cap>limit/2){cap=limit;break;}cap*=2;}unsigned char*p=realloc(b->data,cap?cap:1);if(!p)return ENOMEM;b->data=p;b->capacity=cap;return 0;}
int tool_unhex(const char*s,struct tool_buffer*b,size_t limit){size_t n=strlen(s);if(n%2||n/2>limit)return EINVAL;int rc=reserve(b,n/2,limit);if(rc)return rc;b->size=0;for(size_t i=0;i<n;i+=2){int a=nibble((unsigned char)s[i]),z=nibble((unsigned char)s[i+1]);if(a<0||z<0)return EINVAL;b->data[b->size++]=(unsigned char)(16*a+z);}return 0;}
static int header_line(FILE*f,char*s,size_t cap){size_t n=0;int c;while((c=fgetc(f))!=EOF){if(c==0)return -EINVAL;if(c=='\n'){if(n&&s[n-1]=='\r')n--;s[n]=0;line_number++;return 1;}if(n+1>=cap)return -E2BIG;s[n++]=(char)c;}if(ferror(f))return -EIO;return n?-EINVAL:0;}
static int number(const char*s,size_t*out){if(!*s)return EINVAL;size_t n=0;for(;*s;s++){if(*s<'0'||*s>'9'||n>(SIZE_MAX-(unsigned)(*s-'0'))/10)return EINVAL;n=n*10+(unsigned)(*s-'0');}*out=n;return 0;}
static int page_size(size_t n){return n>=256&&n<=65536&&(n&(n-1))==0;}
struct input_field {
 struct tool_buffer memory;
 FILE *spool;
 size_t size;
};
static void reset_field(struct input_field *field) {
 if (field->spool) fclose(field->spool);
 field->spool = NULL;
 field->size = field->memory.size = 0;
}
static void free_field(struct input_field *field) {
 reset_field(field);
 free(field->memory.data);
}
/* Only this command owns this random, mode-0600 file. Unlink immediately:
 * malformed input, a killed process, or a failing database put leaves no path. */
static int open_spool(FILE **out) {
#ifdef _WIN32
 return tool_spool_open(out);
#else
 const char *directory = getenv("TMPDIR");
 if (!directory || !*directory) directory = "/tmp";
 size_t length = strlen(directory);
 if (length > SIZE_MAX - 32) return ENAMETOOLONG;
 char *path = malloc(length + 32);
 if (!path) return ENOMEM;
 snprintf(path, length + 32, "%s/ulmdb-tool-field-XXXXXX", directory);
 int fd;
#if defined(__APPLE__)
 fd = mkstemp(path);
 if (fd >= 0) {
  int flags = fcntl(fd, F_GETFD);
  if (flags < 0 || fcntl(fd, F_SETFD, flags | FD_CLOEXEC) < 0) {
   int rc = errno; close(fd); (void)unlink(path); free(path); return rc;
  }
 }
#else
 fd = mkostemp(path, O_CLOEXEC);
#endif
 if (fd < 0) { int rc = errno; free(path); return rc; }
 if (unlink(path)) { int rc = errno; close(fd); free(path); return rc; }
 free(path);
 FILE *file = fdopen(fd, "w+b");
 if (!file) { int rc = errno; close(fd); return rc; }
 *out = file;
 return 0;
#endif
}
static int append_field(struct input_field *field, const unsigned char *bytes,
                        size_t n, size_t limit, int may_spool) {
 if (field->size > limit || n > limit - field->size) return E2BIG;
 size_t next = field->size + n;
 if (!field->spool && may_spool && next > TOOL_FIELD_MEMORY) {
  int rc = open_spool(&field->spool);
  if (rc) return rc;
  if (field->memory.size && fwrite(field->memory.data, 1, field->memory.size,
                                    field->spool) != field->memory.size)
   return errno ? errno : EIO;
  free(field->memory.data);
  memset(&field->memory, 0, sizeof field->memory);
 }
 if (field->spool) {
  if (fwrite(bytes, 1, n, field->spool) != n) return errno ? errno : EIO;
 } else {
  int rc = reserve(&field->memory, next, may_spool ? TOOL_FIELD_MEMORY : limit);
  if (rc) return rc;
  if (n) memcpy(field->memory.data + field->size, bytes, n);
  field->memory.size = next;
 }
 field->size = next;
 return 0;
}
/* 1=row, 0=DATA=END, negative=malformed/I/O. EOF is never an implicit footer.
 * Decode only a bounded block; no line or full value allocation is made. */
static int record(FILE *f, struct input_field *field, size_t limit, int may_spool) {
 unsigned char bytes[TOOL_IO_BYTES];
 size_t n = 0;
 reset_field(field);
 int c = getc_unlocked(f);
 if (c == 'D') {
  char tail[16];
  if (!fgets(tail, sizeof tail, f)) return ferror(f) ? -EIO : -EINVAL;
  line_number++;
  return strcmp(tail, "ATA=END\n") && strcmp(tail, "ATA=END\r\n") ? -EINVAL : 0;
 }
 if (c != ' ') return ferror(f) ? -EIO : -EINVAL;
 for (;;) {
  int a = getc_unlocked(f);
  if (a == '\n' || a == '\r') {
   if (a == '\r' && getc_unlocked(f) != '\n') return -EINVAL;
   int rc = append_field(field, bytes, n, limit, may_spool);
   if (rc) return -rc;
   line_number++;
   return 1;
  }
  int z = getc_unlocked(f);
  a = nibble(a);
  z = nibble(z);
  if (a < 0 || z < 0) return ferror(f) ? -EIO : -EINVAL;
  bytes[n++] = (unsigned char)(a * 16 + z);
  if (n == sizeof bytes) {
   int rc = append_field(field, bytes, n, limit, may_spool);
   if (rc) return -rc;
   n = 0;
  }
 }
}
/* Native-created sources may lack a saved LMDB map size. Do not emit a
 * header smaller than their actual mapped inventory; leave bounded metadata
 * headroom for the fresh logical import and reject arithmetic overflow. */
static int dump_mapsize(const MDB_envinfo *info, size_t pagesize, size_t *out) {
 if (!pagesize || info->me_last_pgno > (SIZE_MAX-65536u)/pagesize-1u) return EOVERFLOW;
 size_t actual=(info->me_last_pgno+1u)*pagesize+65536u;
 *out=info->me_mapsize>actual?info->me_mapsize:actual;
 return 0;
}
static int dump_one(MDB_txn*t,struct tool_database*db,int lmdb){
 MDB_cursor*c=NULL;MDB_val k,v;MDB_stat stats;int rc=mdb_stat(t,db->dbi,&stats);if(rc)return rc;
 if(lmdb){if(db->name){for(size_t i=0;i<db->length;i++)if((unsigned char)db->name[i]<32||(unsigned char)db->name[i]>126)return ENOTSUP;}
  if(printf("VERSION=3\nformat=bytevalue\n")<0)return EIO;
  if(db->name&&printf("database=%s\n",db->name)<0)return EIO;
  if(printf("type=btree\n")<0)return EIO;
  MDB_envinfo info;if((rc=mdb_env_info(mdb_txn_env(t),&info)))return rc;
  size_t mapsize;if((rc=dump_mapsize(&info,stats.ms_psize,&mapsize)))return rc;
  if(printf("mapsize=%zu\nmaxreaders=%u\n",mapsize,info.me_maxreaders)<0)return EIO;
  if(db->flags&MDB_DUPSORT&&printf("duplicates=1\n")<0)return EIO;
  for(size_t i=0;i<sizeof flags/sizeof *flags;i++)if(db->flags&flags[i].bit)if(printf("%s=1\n",flags[i].name)<0)return EIO;
  if(printf("db_pagesize=%u\nHEADER=END\n",stats.ms_psize)<0)return EIO;
 }else{if(printf("DB=%s:%08x:",db->name?"NAMED":"MAIN",db->flags)<0||(rc=tool_hex(stdout,db->name,db->length)))return rc?rc:EIO;if(putchar('\n')==EOF)return EIO;}
 if((rc=mdb_cursor_open(t,db->dbi,&c)))return rc;
 while((rc=mdb_cursor_get(c,&k,&v,MDB_NEXT))==0){if(!db->name&&mdb_cursor_is_db(c))continue;if(putchar(' ')==EOF||(rc=tool_hex(stdout,k.mv_data,k.mv_size))||putchar('\n')==EOF||putchar(' ')==EOF||(rc=tool_hex(stdout,v.mv_data,v.mv_size))||putchar('\n')==EOF){if(!rc)rc=EIO;break;}}
 mdb_cursor_close(c);if(rc!=MDB_NOTFOUND)return rc;if(fputs("DATA=END\n",stdout)==EOF)return EIO;return 0;
}
int tool_dump(MDB_env*e,const char*name,int lmdb){
 MDB_txn*t=NULL;struct tool_database*dbs=NULL;size_t n=0;int rc=mdb_txn_begin(e,NULL,MDB_RDONLY,&t);if(rc)return rc;
 if(name){dbs=calloc(1,sizeof *dbs);if(!dbs){rc=ENOMEM;goto done;}dbs[0].name=strdup(name);if(!dbs[0].name){rc=ENOMEM;goto done;}dbs[0].length=strlen(name);n=1;if((rc=mdb_dbi_open(t,name,0,&dbs[0].dbi))||(rc=mdb_dbi_flags(t,dbs[0].dbi,&dbs[0].flags)))goto done;}
 else if(lmdb){dbs=calloc(1,sizeof *dbs);if(!dbs){rc=ENOMEM;goto done;}n=1;if((rc=mdb_dbi_open(t,NULL,0,&dbs[0].dbi))||(rc=mdb_dbi_flags(t,dbs[0].dbi,&dbs[0].flags)))goto done;}
 else if((rc=tool_databases(t,&dbs,&n)))goto done;
 if(!lmdb){MDB_envinfo info;MDB_stat stat;if((rc=mdb_env_info(e,&info))||(rc=mdb_env_stat(e,&stat)))goto done;size_t mapsize;if((rc=dump_mapsize(&info,stat.ms_psize,&mapsize)))goto done;if(printf("ULMDB-DUMP=2\npagesize=%u\nmapsize=%zu\nHEADER=END\n",stat.ms_psize,mapsize)<0){rc=EIO;goto done;}}
 for(size_t i=0;i<n;i++)if((rc=dump_one(t,&dbs[i],lmdb)))goto done;
 if(!lmdb&&fputs("DUMP=END\n",stdout)==EOF)rc=EIO;
 if(!rc&&(fflush(stdout)||ferror(stdout)))rc=EIO;
done:tool_free_databases(dbs,n);mdb_txn_abort(t);return rc;
}
static int lmdb_header(FILE*f,char*line,struct header*h){
 int format=0,type=0,db=0,ps=0,ms=0,dups=0;unsigned seen=0;h->flags=0;h->pagesize=4096;h->mapsize=0;
 if(strcmp(line,"VERSION=3"))return bad("expected VERSION=3");
 for(;;){int got=header_line(f,line,TOOL_MAX_HEADER);if(got!=1)return bad("truncated header");if(!strcmp(line,"HEADER=END"))break;char*eq=strchr(line,'=');if(!eq)return bad("invalid header line");*eq++=0;
  if(!strcmp(line,"format")){if(format++||strcmp(eq,"bytevalue"))return bad("only bytevalue format is supported");}
  else if(!strcmp(line,"type")){if(type++||strcmp(eq,"btree"))return bad("only btree type is supported");}
  else if(!strcmp(line,"database")){if(db++||strlen(eq)>65536)return bad("invalid/repeated database");h->name=strdup(eq);if(!h->name)return ENOMEM;}
  else if(!strcmp(line,"db_pagesize")){if(ps++||number(eq,&h->pagesize)||!page_size(h->pagesize))return bad("invalid page size");}
  else if(!strcmp(line,"mapsize")){if(ms++||number(eq,&h->mapsize)||!h->mapsize)return bad("invalid mapsize");}
  else if(!strcmp(line,"duplicates")){if(dups++||strcmp(eq,"1"))return bad("invalid duplicates marker");h->flags|=MDB_DUPSORT;}
  else if(!strcmp(line,"maxreaders")){size_t x;if(number(eq,&x)||!x)return bad("invalid maxreaders");}
  else if(!strcmp(line,"mapaddr")){if(strncmp(eq,"0x",2)||!* (eq+2))return bad("invalid mapaddr");for(char*p=eq+2;*p;p++)if(nibble(*p)<0)return bad("invalid mapaddr");/* fixed addresses are intentionally not restored */}
  else{size_t i;for(i=0;i<sizeof flags/sizeof *flags;i++)if(!strcmp(line,flags[i].name))break;if(i==sizeof flags/sizeof *flags||(seen&(1u<<i))||(strcmp(eq,"0")&&strcmp(eq,"1")))return bad("unknown/repeated flag");seen|=1u<<i;if(*eq=='1')h->flags|=flags[i].bit;}
 }
 return format&&type?0:bad("missing type/format");
}
static int native_database(char*line,struct header*h,int version){
 size_t flags_at=3;int named=-1;
 if(version==2){
  if(!strncmp(line,"DB=MAIN:",8)){flags_at=8;named=0;}
  else if(!strncmp(line,"DB=NAMED:",9)){flags_at=9;named=1;}
  else return bad("expected explicit MAIN/NAMED database kind");
 }else if(strncmp(line,"DB=",3))return bad("expected DB flags:name header");
 if(strlen(line)<flags_at+9||line[flags_at+8]!=':')return bad("invalid DB flags:name header");
 unsigned bits=0;for(size_t i=flags_at;i<flags_at+8;i++){int n=nibble(line[i]);if(n<0)return bad("invalid database flags");bits=(bits<<4)|(unsigned)n;}
 h->flags=bits;struct tool_buffer b={0};int rc=tool_unhex(line+flags_at+9,&b,65536);
 if(rc){free(b.data);return bad("invalid hex database name");}
 if(!named&&b.size){free(b.data);return bad("MAIN cannot have a name");}
 if(b.size||named==1){
  if(b.size&&memchr(b.data,0,b.size)){free(b.data);return bad("NUL database name");}
  h->name=malloc(b.size+1);if(!h->name){free(b.data);return ENOMEM;}
  if(b.size)memcpy(h->name,b.data,b.size);
  h->name[b.size]=0;
 }
 free(b.data);return 0;
}
static int load_database(MDB_env*e,MDB_txn*t,const struct header*h){
 if(h->flags&~TOOL_DB_FLAGS||((h->flags&(MDB_DUPFIXED|MDB_INTEGERDUP|MDB_REVERSEDUP))&&!(h->flags&MDB_DUPSORT)))return bad("invalid database flag combination");
 MDB_dbi db;int rc=mdb_dbi_open(t,h->name,h->flags|MDB_CREATE,&db);if(rc)return rc;unsigned actual;if((rc=mdb_dbi_flags(t,db,&actual)))return rc;if(actual!=h->flags)return MDB_INCOMPATIBLE;
 struct input_field key={0},value={0};
 size_t maxkey=(size_t)mdb_env_get_maxkeysize(e);
 int got;
 while ((got=record(stdin,&key,maxkey,0))==1) {
  if (!key.size) { rc=bad("empty key"); goto done; }
  int duplicate=(h->flags&MDB_DUPSORT)!=0;
  got=record(stdin,&value,duplicate?maxkey:TOOL_MAX_VALUE,!duplicate);
  if (got!=1) { rc=bad("missing/invalid value record"); goto done; }
  MDB_val k={key.size,key.memory.data};
  MDB_val v={value.size,value.size?(void*)value.memory.data:(void*)""};
  if (value.spool) {
   /* Parsing (including the terminating newline) is complete before reserving.
    * There are no MDB calls while this private destination pointer is filled. */
#ifdef _WIN32
   if (fflush(value.spool)) { rc=errno?errno:EIO; goto done; }
   if ((rc=mdb_put(t,db,&k,&v,MDB_RESERVE))) goto done;
   if ((rc=tool_spool_copy(value.spool,v.mv_data,value.size))) goto done;
#else
   if (fflush(value.spool) || fseeko(value.spool,0,SEEK_SET)) { rc=errno?errno:EIO; goto done; }
   if ((rc=mdb_put(t,db,&k,&v,MDB_RESERVE))) goto done;
   size_t offset=0;
   while (offset<value.size) {
    size_t amount=value.size-offset;
    if (amount>TOOL_IO_BYTES) amount=TOOL_IO_BYTES;
    if (fread((unsigned char*)v.mv_data+offset,1,amount,value.spool)!=amount) { rc=EIO; goto done; }
    offset+=amount;
   }
#endif
  } else if ((rc=mdb_put(t,db,&k,&v,0))) goto done;
 }
 rc=got==0?0:bad("invalid/truncated key record");
 done:free_field(&key);free_field(&value);return rc;
}
/* Fixed capacity covers MAIN plus every public user slot. Hash collisions
 * still compare complete names; no alias spelling or flags are normalized. */
static size_t name_slot(const char *name, const struct tool_database *seen,
                        const uint32_t *slots, int *duplicate) {
 uint64_t hash=UINT64_C(14695981039346656037);
 if (name) for (const unsigned char *p=(const unsigned char*)name;*p;p++) {
  hash^=*p;hash*=UINT64_C(1099511628211);
 }
 size_t slot=(size_t)hash&(TOOL_NAME_SLOTS-1u);
 while (slots[slot]) {
  const char *old=seen[slots[slot]-1u].name;
  if ((!name&&!old)||(name&&old&&!strcmp(name,old))) { *duplicate=1; break; }
  slot=(slot+1u)&(TOOL_NAME_SLOTS-1u);
 }
 return slot;
}
int tool_load(const char*target,size_t requested_map){
 char*line=malloc(TOOL_MAX_HEADER),*path=NULL;int owned=0,rc=0,native=0;MDB_env*e=NULL;MDB_txn*t=NULL;struct header h={0};struct tool_database*seen=calloc(TOOL_MAX_DBS+1u,sizeof *seen);size_t count=0;uint32_t *slots=calloc(TOOL_NAME_SLOTS,sizeof *slots);line_number=0;
 if(!line||!seen||!slots){rc=ENOMEM;goto done;}
 int got=header_line(stdin,line,TOOL_MAX_HEADER);if(got!=1){rc=bad("empty/truncated input");goto done;}
 if(!strcmp(line,"ULMDB-DUMP=1")||!strcmp(line,"ULMDB-DUMP=2")){native=line[11]-'0';int ps=0,ms=0;h.pagesize=4096;
  for(;;){if(header_line(stdin,line,TOOL_MAX_HEADER)!=1){rc=bad("truncated native header");goto done;}if(!strcmp(line,"HEADER=END"))break;if(!strncmp(line,"pagesize=",9)){if(ps++||number(line+9,&h.pagesize)||!page_size(h.pagesize)){rc=bad("invalid page size");goto done;}}else if(!strncmp(line,"mapsize=",8)){if(ms++||number(line+8,&h.mapsize)||!h.mapsize){rc=bad("invalid mapsize");goto done;}}else{rc=bad("unknown native header");goto done;}}
  if(!ps||!ms){rc=bad("missing native header fields");goto done;}
 }else if((rc=lmdb_header(stdin,line,&h)))goto done;
 size_t geometry=h.pagesize;
 if((rc=tool_prepare_target(target,&path,&owned)))goto done;
 if((rc=tool_open(path,MDB_WRITEMAP,requested_map?requested_map:(h.mapsize?h.mapsize:1024u*1024u*1024u),geometry,&e)))goto done;
 MDB_stat stat;if((rc=mdb_env_stat(e,&stat)))goto done;if(stat.ms_psize!=geometry){rc=bad("target page size differs");goto done;}
 if((rc=mdb_txn_begin(e,NULL,0,&t)))goto done;
 for(;;){
  if(native){if(header_line(stdin,line,TOOL_MAX_HEADER)!=1){rc=bad("missing DUMP=END");goto done;}if(!strcmp(line,"DUMP=END")){if(!count||header_line(stdin,line,TOOL_MAX_HEADER)!=0){rc=bad("trailing data/empty dump");goto done;}break;}if((rc=native_database(line,&h,native)))goto done;}
  if(count==TOOL_MAX_DBS+1u){rc=MDB_DBS_FULL;goto done;}
  int duplicate=0;size_t slot=name_slot(h.name,seen,slots,&duplicate);
  if(duplicate){rc=bad("repeated database section");goto done;}
  slots[slot]=(uint32_t)count+1u;seen[count++].name=h.name;h.name=NULL;struct header current=h;current.name=seen[count-1].name;
  if((rc=load_database(e,t,&current)))goto done;
  if(!native){got=header_line(stdin,line,TOOL_MAX_HEADER);if(got==0)break;if(got!=1){rc=bad("truncated next header");goto done;}if((rc=lmdb_header(stdin,line,&h)))goto done;if(h.pagesize!=geometry){rc=bad("page size changed between sections");goto done;}}
 }
 {MDB_txn*commit=t;t=NULL;rc=mdb_txn_commit(commit);}if(rc)goto done;mdb_env_close(e);e=NULL;
 if(owned){rc=tool_publish_target(path,target);if(!rc)owned=0;}
done:if(t)mdb_txn_abort(t);if(e)mdb_env_close(e);if(owned&&path)tool_remove_owned(path);free(path);free(h.name);tool_free_databases(seen,count);free(slots);free(line);return rc;
}
