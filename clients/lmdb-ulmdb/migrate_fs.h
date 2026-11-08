/* Filesystem ownership and atomic publication for the standalone importer. */
#ifndef ULMDB_MIGRATE_FS_H
#define ULMDB_MIGRATE_FS_H
#include "migrate_stream.h"
#include <stddef.h>

int migration_error(char *, size_t, const char *, ...);
char *migration_join(const char *, const char *);
int migration_source_abi(const char *, const char **, int *, char *, size_t);
int migration_destination(const char *, char **, char **, char *, size_t);
int migration_metadata(const char *, const char *, const char *,
                       const struct migration_info *, char *, size_t);
int migration_publish(const char *, const char *, int *, char *, size_t);
int migration_remove_stage(const char *, char *, size_t);
#endif
