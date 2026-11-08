#ifndef ULMDB_OPERATOR_WINDOWS_H
#define ULMDB_OPERATOR_WINDOWS_H
#ifdef _WIN32
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0602
#endif
#include <windows.h>
#include <winternl.h>
#include <lmdb.h>
#include <stdio.h>
int tool_windows_error(DWORD);
int tool_platform_init(void);
int tool_stdio_handle(FILE *, mdb_filehandle_t *);
int tool_spool_open(FILE **);
int tool_spool_copy(FILE *, void *, size_t);
wchar_t *tool_windows_wide(const char *);
char *tool_windows_utf8(const wchar_t *);
int tool_windows_private_descriptor(PSECURITY_DESCRIPTOR *);
int tool_windows_random(void *, size_t);
int tool_windows_complete(HANDLE, NTSTATUS, IO_STATUS_BLOCK *);
int tool_windows_identity(HANDLE, FILE_ID_INFO *, int);
int tool_windows_relative(HANDLE,const wchar_t *,int,int,ACCESS_MASK,PSECURITY_DESCRIPTOR,HANDLE *);
int tool_windows_sync_directory(HANDLE);
#endif
#endif
