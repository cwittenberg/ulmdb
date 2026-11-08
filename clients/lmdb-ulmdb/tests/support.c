/* Test-owned filesystem state and subprocesses; never shells application args. */
#include "support.h"

#include <errno.h>
#include <fcntl.h>
#include <ftw.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <wordexp.h>

static void report(const char *format, va_list arguments) {
    fputs("FAIL: ", stderr);
    vfprintf(stderr, format, arguments);
    fputc('\n', stderr);
    exit(1);
}

void fail(const char *format, ...) {
    va_list arguments;
    va_start(arguments, format);
    report(format, arguments);
    va_end(arguments);
}

void require(int condition, const char *format, ...) {
    if (!condition) {
        va_list arguments;
        va_start(arguments, format);
        report(format, arguments);
        va_end(arguments);
    }
}

char *path_join(const char *left, const char *right) {
    char *result = NULL;
    if (asprintf(&result, "%s/%s", left, right) < 0) fail("allocate path");
    return result;
}

char *absolute_path(const char *path) {
    if (path[0] == '/') {
        char *result = strdup(path);
        require(result != NULL, "allocate absolute path");
        return result;
    }
    char *cwd = getcwd(NULL, 0);
    require(cwd != NULL, "getcwd: %s", strerror(errno));
    char *result = path_join(cwd, path);
    free(cwd);
    return result;
}

int path_exists(const char *path) {
    struct stat status;
    if (lstat(path, &status) == 0) return 1;
    require(errno == ENOENT || errno == ENOTDIR, "lstat %s: %s", path, strerror(errno));
    return 0;
}

void make_directory(const char *path) {
    require(mkdir(path, 0700) == 0, "mkdir %s: %s", path, strerror(errno));
}

static int remove_entry(const char *path, const struct stat *status, int type, struct FTW *walk) {
    (void)status;
    (void)type;
    (void)walk;
    return remove(path);
}

void remove_tree(const char *path) {
    if (path_exists(path)) {
        require(nftw(path, remove_entry, 32, FTW_DEPTH | FTW_PHYS) == 0,
                "remove owned test tree %s: %s", path, strerror(errno));
    }
}

void write_file(const char *path, const void *data, size_t length) {
    FILE *file = fopen(path, "wb");
    require(file != NULL, "write %s: %s", path, strerror(errno));
    require(!length || fwrite(data, 1, length, file) == length, "write bytes %s", path);
    require(fclose(file) == 0, "close %s", path);
}

char *read_file(const char *path, size_t *length) {
    FILE *file = fopen(path, "rb");
    require(file != NULL, "read %s: %s", path, strerror(errno));
    require(fseek(file, 0, SEEK_END) == 0, "seek %s", path);
    long end = ftell(file);
    require(end >= 0 && (uintmax_t)end < SIZE_MAX, "file size %s", path);
    require(fseek(file, 0, SEEK_SET) == 0, "rewind %s", path);
    char *contents = malloc((size_t)end + 1);
    require(contents != NULL, "allocate %ld bytes", end);
    require(!end || fread(contents, 1, (size_t)end, file) == (size_t)end, "read bytes %s", path);
    require(fclose(file) == 0, "close %s", path);
    contents[end] = '\0';
    if (length) *length = (size_t)end;
    return contents;
}

void copy_file(const char *source, const char *destination) {
    FILE *input = fopen(source, "rb"), *output = fopen(destination, "wb");
    require(input != NULL && output != NULL, "copy %s to %s: %s", source, destination, strerror(errno));
    unsigned char bytes[65536];
    size_t count;
    while ((count = fread(bytes, 1, sizeof bytes, input)) != 0) {
        require(fwrite(bytes, 1, count, output) == count, "copy write %s", destination);
    }
    require(!ferror(input), "copy read %s", source);
    require(fclose(input) == 0 && fclose(output) == 0, "copy close %s", destination);
}

void require_same_file(const char *left, const char *right) {
    FILE *a = fopen(left, "rb"), *b = fopen(right, "rb");
    require(a != NULL && b != NULL, "open file comparison %s and %s", left, right);
    unsigned char a_bytes[65536], b_bytes[65536];
    size_t a_count, b_count;
    do {
        a_count = fread(a_bytes, 1, sizeof a_bytes, a);
        b_count = fread(b_bytes, 1, sizeof b_bytes, b);
        require(a_count == b_count && memcmp(a_bytes, b_bytes, a_count) == 0,
                "file changed: %s differs from snapshot %s", left, right);
    } while (a_count);
    require(!ferror(a) && !ferror(b), "read file comparison %s", left);
    require(fclose(a) == 0 && fclose(b) == 0, "close file comparison %s", left);
}

static void apply_environment(const char *const entries[]) {
    for (size_t i = 0; entries && entries[i]; ++i) {
        char *entry = strdup(entries[i]);
        if (!entry) _exit(126);
        char *equals = strchr(entry, '=');
        if (equals) {
            *equals = '\0';
            if (setenv(entry, equals + 1, 1) != 0) _exit(126);
        } else if (unsetenv(entry) != 0) {
            _exit(126);
        }
        free(entry);
    }
}

int run_command(const char *const arguments[], const char *const environment[], char **output) {
    char log[] = "/tmp/ulmdb-native-test-output-XXXXXX";
    int descriptor = mkstemp(log);
    require(descriptor >= 0, "create process log: %s", strerror(errno));
    pid_t child = fork();
    require(child >= 0, "fork: %s", strerror(errno));
    if (child == 0) {
        (void)setpgid(0, 0);
        if (dup2(descriptor, STDOUT_FILENO) < 0 || dup2(descriptor, STDERR_FILENO) < 0) _exit(126);
        close(descriptor);
        apply_environment(environment);
        execvp(arguments[0], (char *const *)arguments);
        perror(arguments[0]);
        _exit(127);
    }
    close(descriptor);
    struct timespec start, now, pause = {0, 10000000};
    require(clock_gettime(CLOCK_MONOTONIC, &start) == 0, "read monotonic clock");
    int status;
    for (;;) {
        pid_t result = waitpid(child, &status, WNOHANG);
        if (result == child) break;
        require(result == 0 || errno == EINTR, "waitpid: %s", strerror(errno));
        require(clock_gettime(CLOCK_MONOTONIC, &now) == 0, "read process deadline");
        if (now.tv_sec - start.tv_sec > 120) {
            (void)kill(-child, SIGKILL);
            (void)kill(child, SIGKILL);
            (void)waitpid(child, &status, 0);
            fail("command exceeded 120 seconds: %s (log %s)", arguments[0], log);
        }
        (void)nanosleep(&pause, NULL);
    }
    char *contents = read_file(log, NULL);
    require(unlink(log) == 0, "remove command log %s", log);
    if (output) *output = contents;
    else free(contents);
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}

void checked_command(const char *const arguments[], const char *const environment[]) {
    char *output;
    int status = run_command(arguments, environment, &output);
    if (status) fail("command %s exited %d:\n%s", arguments[0], status, output);
    free(output);
}

void compile_command(const char *const arguments[]) {
    const char *compiler = getenv("CC");
    wordexp_t words;
    int parsed = wordexp(compiler && *compiler ? compiler : "cc", &words, WRDE_NOCMD);
    require(parsed == 0 && words.we_wordc > 0, "parse CC without command substitution");
    size_t count = 0;
    while (arguments[count]) ++count;
    const char **command = calloc(words.we_wordc + count + 1, sizeof *command);
    require(command != NULL, "allocate compiler command");
    for (size_t i = 0; i < words.we_wordc; ++i) command[i] = words.we_wordv[i];
    for (size_t i = 0; i < count; ++i) command[words.we_wordc + i] = arguments[i];
    checked_command(command, NULL);
    free(command);
    wordfree(&words);
}
