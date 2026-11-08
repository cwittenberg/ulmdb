#ifndef ULMDB_WINDOWS_PROCESS_H
#define ULMDB_WINDOWS_PROCESS_H
#ifdef _WIN32
#include "operator_windows.h"
struct windows_child {HANDLE process,job;DWORD pid;ULONGLONG deadline;};
int windows_child_start(const wchar_t *,const wchar_t *const *,HANDLE,HANDLE,struct windows_child *,char *,size_t);
int windows_application_start(const wchar_t *,const wchar_t *const *,struct windows_child *,char *,size_t);
int windows_child_wait(struct windows_child *,DWORD *,char *,size_t);
void windows_child_stop(struct windows_child *);
wchar_t *windows_executable_directory(void);
wchar_t *windows_fullpath(const wchar_t *);
#endif
#endif
