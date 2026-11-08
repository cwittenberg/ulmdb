#ifdef _WIN32
#include "migrate_windows.c"
#else
/* Standalone LMDB-to-ULMDB import. No original LMDB symbols enter this process. */
#include "migrate_fs.h"
#include "migrate_stream.h"
#include <errno.h>
#include <fcntl.h>
#include <getopt.h>
#include <inttypes.h>
#include <limits.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#if defined(__APPLE__)
#include "darwin_tools.h"
#endif

struct options {
    const char *source, *destination, *library, *exporter;
    int nosubdir, quiet;
};
static void usage(FILE *output) {
    fputs("usage: ulmdb-migrate SOURCE DESTINATION [--library FILE] [--exporter FILE]\n"
          "                     [--nosubdir] [--quiet]\n"
          "Import a consistent LMDB snapshot into a new native ULMDB directory.\n"
          "The source is preserved; DESTINATION must be absent.\n", output);
}
static int parse_options(int argc, char **argv, struct options *options) {
    static const struct option arguments[] = {
        {"library", required_argument, NULL, 'l'},
        {"exporter", required_argument, NULL, 'e'},
        {"nosubdir", no_argument, NULL, 'n'},
        {"quiet", no_argument, NULL, 'q'},
        {"help", no_argument, NULL, 'h'}, {NULL, 0, NULL, 0}
    };
    int option, positionals = 0;
    memset(options, 0, sizeof *options);
    /* RETURN_IN_ORDER keeps options after SOURCE/DESTINATION working even when
       POSIXLY_CORRECT is inherited by an unchanged application. */
    while ((option = getopt_long(argc, argv, "-h", arguments, NULL)) != -1) {
        switch (option) {
        case 1:
            if (!positionals) options->source = optarg;
            else if (positionals == 1) options->destination = optarg;
            else { usage(stderr); return -1; }
            ++positionals;
            break;
        case 'l': options->library = optarg; break;
        case 'e': options->exporter = optarg; break;
        case 'n': options->nosubdir = 1; break;
        case 'q': options->quiet = 1; break;
        case 'h': usage(stdout); return 1;
        default: usage(stderr); return -1;
        }
    }
    while (optind < argc) {
        if (!positionals) options->source = argv[optind++];
        else if (positionals == 1) options->destination = argv[optind++];
        else { usage(stderr); return -1; }
        ++positionals;
    }
    if (positionals != 2) { usage(stderr); return -1; }
    return 0;
}
static char *executable_directory(char *error, size_t size) {
#if defined(__APPLE__)
    char *executable = ulmdb_tool_executable_path();
    if (!executable) {
        migration_error(error, size, "locate migration executable: %s", strerror(errno));
        return NULL;
    }
#else
    char executable[PATH_MAX + 1];
    ssize_t count = readlink("/proc/self/exe", executable, sizeof executable - 1);
    if (count < 0 || (size_t)count >= sizeof executable - 1) {
        migration_error(error, size, "locate migration executable: %s", count < 0 ? strerror(errno) : "path too long");
        return NULL;
    }
    executable[count] = 0;
#endif
    char *last = strrchr(executable, '/');
    if (!last) {
#if defined(__APPLE__)
        free(executable);
#endif
        migration_error(error, size, "migration executable has no directory"); return NULL;
    }
    if (last == executable) last[1] = 0;
    else *last = 0;
    char *result = strdup(executable);
#if defined(__APPLE__)
    free(executable);
#endif
    if (!result) migration_error(error, size, "allocate executable directory: %s", strerror(errno));
    return result;
}
static int start_exporter(const char *exporter, const char *source, int nosubdir,
                           pid_t *pid, int *input, char *error, size_t size) {
    int descriptors[2];
#if defined(__APPLE__)
    if (ulmdb_tool_pipe_cloexec(descriptors))
#else
    if (pipe2(descriptors, O_CLOEXEC))
#endif
        return migration_error(error, size, "create exporter pipe: %s", strerror(errno));
    pid_t child = fork();
    if (child < 0) {
        int saved = errno;
        close(descriptors[0]); close(descriptors[1]);
        return migration_error(error, size, "start LMDB exporter: %s", strerror(saved));
    }
    if (!child) {
        close(descriptors[0]);
        if (dup2(descriptors[1], STDOUT_FILENO) < 0 ||
            fcntl(STDOUT_FILENO, F_SETFD, 0) < 0 || unsetenv("LD_PRELOAD")
#if defined(__APPLE__)
            || unsetenv("DYLD_INSERT_LIBRARIES") || unsetenv("DYLD_FORCE_FLAT_NAMESPACE")
#endif
            ) {
            perror("ulmdb-migrate: prepare LMDB exporter"); _exit(127);
        }
        if (descriptors[1] != STDOUT_FILENO) close(descriptors[1]);
        char *const arguments[] = {(char *)exporter, (char *)source,
                                    nosubdir ? "--nosubdir" : NULL, NULL};
        execv(exporter, arguments);
        perror("ulmdb-migrate: execute LMDB exporter");
        _exit(127);
    }
    close(descriptors[1]);
    *pid = child;
    *input = descriptors[0];
    return 0;
}
static int wait_exporter(pid_t pid, char *error, size_t size) {
    struct timespec started, current, pause = {0, 10000000};
    if (clock_gettime(CLOCK_MONOTONIC, &started))
        return migration_error(error, size, "read exporter deadline: %s", strerror(errno));
    for (;;) {
        int status;
        pid_t result = waitpid(pid, &status, WNOHANG);
        if (result < 0 && errno == EINTR) continue;
        if (result < 0) return migration_error(error, size, "wait for LMDB exporter: %s", strerror(errno));
        if (result == pid) {
            if (WIFEXITED(status) && !WEXITSTATUS(status)) return 0;
            return migration_error(error, size, "LMDB snapshot exporter failed with status %d",
                                   WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status));
        }
        if (clock_gettime(CLOCK_MONOTONIC, &current))
            return migration_error(error, size, "read exporter deadline: %s", strerror(errno));
        if (current.tv_sec - started.tv_sec >= 120)
            return migration_error(error, size, "LMDB snapshot exporter did not finish within 120 seconds");
        nanosleep(&pause, NULL);
    }
}
static int migrate(const struct options *options, struct migration_info *info,
                    char *error, size_t size) {
    struct migration_api api = {0};
    MDB_env *env = NULL;
    char *directory = NULL, *source = NULL, *destination = NULL, *stage = NULL;
    char *library = NULL, *exporter = NULL, *previous_abi = NULL;
    const char *abi = NULL;
    int result = -1, single_file = 0, staged = 0, published = 0, input = -1, abi_changed = 0;
    pid_t child = -1;
    source = realpath(options->source, NULL);
    if (!source) { migration_error(error, size, "resolve LMDB source: %s", strerror(errno)); goto done; }
    if (migration_source_abi(source, &abi, &single_file, error, size)) goto done;
    directory = executable_directory(error, size);
    if (!directory) goto done;
    const char *library_override = options->library ? options->library : getenv("ULMDB_LIBRARY");
    library = library_override ? realpath(library_override, NULL) : migration_join(directory,
#if defined(__APPLE__)
        "../lib/liblmdb.0.dylib"
#else
        "../lib/liblmdb.so.0"
#endif
    );
    exporter = options->exporter ? realpath(options->exporter, NULL) : migration_join(directory, "lmdb-export");
    if (!library || !exporter) { migration_error(error, size, "locate migration tools: %s", strerror(errno)); goto done; }
    const char *exporter_name = strrchr(exporter, '/');
    exporter_name = exporter_name ? exporter_name + 1 : exporter;
    if (!strcmp(abi, "0.9") && !strcmp(exporter_name, "lmdb-export")) {
        size_t capacity = strlen(exporter) + 5;
        char *legacy = malloc(capacity);
        if (!legacy) { migration_error(error, size, "allocate legacy exporter path: %s", strerror(errno)); goto done; }
        snprintf(legacy, capacity, "%s-0.9", exporter);
        free(exporter); exporter = legacy;
    }
    if (access(exporter, X_OK)) { migration_error(error, size, "LMDB exporter unavailable: %s: %s", exporter, strerror(errno)); goto done; }
    if (migration_load(&api, library, error, size)) goto done;
    if (migration_destination(options->destination, &destination, &stage, error, size)) goto done;
    staged = 1;
    const char *previous = getenv("ULMDB_LMDB_ABI");
    if (previous) {
        previous_abi = strdup(previous);
        if (!previous_abi) { migration_error(error, size, "save ABI selection: %s", strerror(errno)); goto done; }
    }
    if (setenv("ULMDB_LMDB_ABI", abi, 1)) { migration_error(error, size, "select source ABI: %s", strerror(errno)); goto done; }
    abi_changed = 1;
    if (migration_check(&api, api.env_create(&env), "create destination environment", error, size) ||
        migration_check(&api, api.env_set_maxdbs(env, 65528), "set destination database capacity", error, size) ||
        start_exporter(exporter, source, options->nosubdir || single_file, &child, &input, error, size) ||
        migration_read_header(input, info, error, size) ||
        migration_check(&api, api.env_set_maxreaders(env, info->max_readers), "set destination reader capacity", error, size) ||
        migration_check(&api, api.env_set_pagesize(env, info->page_size), "set destination page geometry", error, size) ||
        migration_check(&api, api.env_set_mapsize(env, (size_t)info->map_size), "set destination map capacity", error, size) ||
        migration_check(&api, api.env_open(env, stage, MDB_WRITEMAP, 0600), "open destination environment", error, size) ||
        migration_replay(&api, env, input, info, error, size)) goto done;
    close(input); input = -1;
    if (wait_exporter(child, error, size)) goto done;
    child = -1;
    api.env_close(env); env = NULL;
    if (migration_metadata(stage, source, abi, info, error, size) ||
        migration_publish(stage, destination, &published, error, size)) goto done;
    result = 0;
done:
    if (env) api.env_close(env);
    if (input >= 0) close(input);
    if (child > 0) {
        /* Only our direct exporter is terminated. Its stdout was closed first. */
        int status;
        pid_t waited;
        do { waited = waitpid(child, &status, WNOHANG); } while (waited < 0 && errno == EINTR);
        if (!waited) {
            kill(child, SIGKILL);
            do { waited = waitpid(child, &status, 0); } while (waited < 0 && errno == EINTR);
        }
    }
    migration_unload(&api);
    if (abi_changed) {
        if (previous_abi) setenv("ULMDB_LMDB_ABI", previous_abi, 1);
        else unsetenv("ULMDB_LMDB_ABI");
    }
    if (staged && !published) {
        char cleanup_error[512];
        if (migration_remove_stage(stage, cleanup_error, sizeof cleanup_error))
            fprintf(stderr, "ulmdb-migrate: %s\n", cleanup_error);
    }
    free(previous_abi); free(exporter); free(library); free(stage);
    free(destination); free(source); free(directory);
    return result;
}
int main(int argc, char **argv) {
    struct options options;
    int parsed = parse_options(argc, argv, &options);
    if (parsed) return parsed < 0 ? 2 : 0;
    struct migration_info info;
    char error[1024];
    if (migrate(&options, &info, error, sizeof error)) {
        fprintf(stderr, "ulmdb-migrate: %s\n", error);
        return 1;
    }
    if (!options.quiet)
        printf("Migrated %" PRIu64 " records across %" PRIu64
               " databases at source transaction %" PRIu64 "\n",
               info.records, info.databases, info.source_sequence);
    return 0;
}

#endif
