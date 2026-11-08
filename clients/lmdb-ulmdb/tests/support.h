/* Small process and filesystem helpers for native adapter integration tests. */
#ifndef ULMDB_ADAPTER_TEST_SUPPORT_H
#define ULMDB_ADAPTER_TEST_SUPPORT_H

#include <stddef.h>

void fail(const char *format, ...);
void require(int condition, const char *format, ...);
char *path_join(const char *left, const char *right);
char *absolute_path(const char *path);
int path_exists(const char *path);
void make_directory(const char *path);
void remove_tree(const char *path);
void write_file(const char *path, const void *data, size_t length);
char *read_file(const char *path, size_t *length);
void copy_file(const char *source, const char *destination);
void require_same_file(const char *left, const char *right);
/* Environment entries use NAME=VALUE to set a variable, or NAME to unset it. */
int run_command(const char *const arguments[], const char *const environment[], char **output);
void checked_command(const char *const arguments[], const char *const environment[]);
void compile_command(const char *const arguments[]);

#endif
