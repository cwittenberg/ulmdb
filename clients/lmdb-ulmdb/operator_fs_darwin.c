/* Darwin publication retains genuine parent/root descriptors through fences. */
#include "operator.h"
#include "darwin_tools.h"
#include <dirent.h>

int tool_prepare_target(const char *target, char **path, int *owned) {
    struct stat state;
    if (!lstat(target, &state)) {
        if (!S_ISDIR(state.st_mode)) return ENOTDIR;
        *path = strdup(target); *owned = 0; return *path ? 0 : ENOMEM;
    }
    if (errno != ENOENT) return errno;
    size_t length = strlen(target);
    if (length > SIZE_MAX - 20) return EOVERFLOW;
    char *temporary = malloc(length + 20);
    if (!temporary) return ENOMEM;
    memcpy(temporary, target, length); memcpy(temporary + length, ".load-XXXXXX", 13);
    if (!mkdtemp(temporary)) { int saved = errno; free(temporary); return saved; }
    *path = temporary; *owned = 1; return 0;
}
void tool_remove_owned(const char *path) {
    DIR *directory = opendir(path);
    if (!directory) return;
    struct dirent *entry;
    while ((entry = readdir(directory))) {
        if (!strcmp(entry->d_name, ".") || !strcmp(entry->d_name, "..")) continue;
        size_t left = strlen(path), right = strlen(entry->d_name);
        if (left > SIZE_MAX - right - 2) continue;
        char *child = malloc(left + right + 2);
        if (!child) continue;
        snprintf(child, left + right + 2, "%s/%s", path, entry->d_name);
        struct stat state;
        if (!lstat(child, &state)) {
            if (S_ISDIR(state.st_mode)) tool_remove_owned(child);
            else (void)unlink(child);
        }
        free(child);
    }
    closedir(directory); (void)rmdir(path);
}
int tool_publish_target(const char *temporary, const char *target) {
    int published = 0;
    return ulmdb_tool_publish_exclusive(temporary, target, &published);
}
