#ifndef ULMDB_MIGRATION_WINDOWS_H
#define ULMDB_MIGRATION_WINDOWS_H
#ifdef _WIN32
#include "windows_process.h"
#include <sys/types.h>
void migration_windows_input_start(struct windows_child *);
ssize_t migration_windows_read(int,void *,size_t);
#endif
#endif
