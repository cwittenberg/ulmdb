#ifndef ULMDB_OPERATOR_H
#define ULMDB_OPERATOR_H
#include <lmdb.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#define TOOL_DB_FLAGS (MDB_REVERSEKEY|MDB_DUPSORT|MDB_INTEGERKEY|MDB_DUPFIXED|MDB_INTEGERDUP|MDB_REVERSEDUP)
/* C extended catalog: 65536 native records minus eight internal slots. */
#define TOOL_MAX_DBS 65528u
/* Exact64-bit MAXIMUM profile bound; output never computes2*length. */
#define TOOL_MAX_VALUE UINT32_MAX
#define TOOL_FIELD_MEMORY (1024u*1024u)
#define TOOL_IO_BYTES 65536u
#define TOOL_NAME_SLOTS 131072u
#define TOOL_MAX_HEADER (2u*65536u+256u)
struct tool_buffer { unsigned char *data; size_t size,capacity; };
struct tool_database { char *name; size_t length; MDB_dbi dbi; unsigned flags; };
int tool_open(const char*,unsigned,size_t,size_t,MDB_env**);
int tool_databases(MDB_txn*,struct tool_database**,size_t*);
void tool_free_databases(struct tool_database*,size_t);
int tool_dump(MDB_env*,const char*,int);
int tool_load(const char*,size_t);
int tool_inspect(MDB_env*,int,int);
int tool_readers(MDB_env*,int,int);
int tool_hex(FILE*,const void*,size_t);
int tool_unhex(const char*,struct tool_buffer*,size_t);
int tool_prepare_target(const char*,char**,int*);
int tool_publish_target(const char*,const char*);
void tool_remove_owned(const char*);
#endif
