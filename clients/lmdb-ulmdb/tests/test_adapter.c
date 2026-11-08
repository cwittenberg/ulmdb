/* Run unchanged real-LMDB executables through the native ULMDB package. */
#include "support.h"
#include "../build_support.h"

#include <dirent.h>
#include <errno.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

enum abi { MODERN = 0, LEGACY = 1 };
struct suite {
    char work[PATH_MAX];
    char *root, *package, *self, *source[2], *upstream[2], *app[2], *saved_app[2];
    char *launcher, *migrator, *library, *exporter;
    unsigned passed;
};
static struct suite suite;

static void cleanup(void) {
    if (suite.work[0]) remove_tree(suite.work);
    free(suite.root);
    free(suite.package);
    free(suite.self);
    free(suite.launcher);
    free(suite.migrator);
    free(suite.library);
    free(suite.exporter);
    for (size_t i = 0; i < 2; ++i) {
        free(suite.source[i]);
        free(suite.upstream[i]);
        free(suite.app[i]);
        free(suite.saved_app[i]);
    }
}

static char *formatted(const char *left, const char *right) {
    char *value = NULL;
    require(asprintf(&value, "%s=%s", left, right) >= 0, "allocate environment setting");
    return value;
}

static const char *abi_name(enum abi abi) { return abi == MODERN ? "1.0" : "0.9"; }

static int application(enum abi abi, const char *action, const char *path, int adapter,
                       const char *extra_environment, char **output) {
    const char *arguments[5];
    size_t count = 0;
    if (adapter) arguments[count++] = suite.launcher;
    arguments[count++] = suite.app[abi];
    arguments[count++] = action;
    arguments[count++] = path;
    arguments[count] = NULL;
    char *library = formatted("LD_LIBRARY_PATH", suite.upstream[abi]);
    char *abi_setting = formatted("ULMDB_LMDB_ABI", abi_name(abi));
    const char *environment[] = {
        library, abi_setting, "ULMDB_MIGRATOR", "LD_PRELOAD", extra_environment, NULL
    };
    int result = run_command(arguments, environment, output);
    free(library);
    free(abi_setting);
    return result;
}

static void app(enum abi abi, const char *action, const char *path, int adapter) {
    char *output;
    int status = application(abi, action, path, adapter, NULL, &output);
    if (status) fail("%s (%s, adapter=%d) exited %d:\n%s", action, abi_name(abi), adapter, status, output);
    free(output);
}

static int migrate(const char *source, const char *destination, const char *exporter,
                   const char *extra_environment, char **output) {
    const char *arguments[] = {
        suite.migrator, source, destination, "--library", suite.library,
        "--exporter", exporter ? exporter : suite.exporter, "--quiet", NULL
    };
    const char *environment[] = {"LD_PRELOAD", extra_environment, NULL};
    return run_command(arguments, environment, output);
}

static void import(const char *source, const char *destination) {
    char *output;
    int status = migrate(source, destination, NULL, NULL, &output);
    if (status) fail("migration exited %d:\n%s", status, output);
    free(output);
}

static void passed(const char *name) {
    ++suite.passed;
    printf("PASS: %s\n", name);
    fflush(stdout);
}

static char *snapshot(const char *source, const char *label) {
    char *saved = path_join(suite.work, label);
    copy_file(source, saved);
    return saved;
}

static void require_text(const char *path, const char *expected) {
    size_t length;
    char *contents = read_file(path, &length);
    require(length == strlen(expected) && memcmp(contents, expected, length) == 0,
            "unexpected contents in %s", path);
    free(contents);
}

static void require_metadata_number(const char *path, const char *name, unsigned long long expected) {
    char *contents = read_file(path, NULL), *key = NULL;
    require(asprintf(&key, "\"%s\"", name) >= 0, "allocate metadata key");
    char *found = strstr(contents, key);
    require(found != NULL, "missing %s in %s", name, path);
    char *colon = strchr(found, ':');
    require(colon != NULL, "missing colon for %s", name);
    char *end;
    errno = 0;
    unsigned long long value = strtoull(colon + 1, &end, 10);
    require(end != colon + 1 && !errno && value == expected,
            "metadata %s expected %llu, got %llu", name, expected, value);
    free(key);
    free(contents);
}

static void require_no_stage(const char *destination) {
    const char *name = strrchr(destination, '/');
    require(name != NULL, "absolute destination path");
    char *prefix = NULL;
    require(asprintf(&prefix, ".%s.ulmdb-import-", name + 1) >= 0, "allocate stage prefix");
    DIR *directory = opendir(suite.work);
    require(directory != NULL, "open test directory");
    struct dirent *entry;
    while ((entry = readdir(directory)) != NULL) {
        require(strncmp(entry->d_name, prefix, strlen(prefix)) != 0,
                "failed migration left private stage %s", entry->d_name);
    }
    require(closedir(directory) == 0, "close test directory");
    free(prefix);
}

static void test_same_binary(void) {
    char *original = path_join(suite.work, "original-dynamic");
    char *native = path_join(suite.work, "native-dynamic");
    make_directory(original);
    make_directory(native);
    char *output;
    require(application(MODERN, "backend", original, 0, NULL, &output) == 0, "reference backend");
    require(strstr(output, "LMDB") != NULL, "reference backend identity");
    free(output);
    require(application(MODERN, "backend", native, 1, NULL, &output) == 0, "adapter backend");
    require(strstr(output, "ULMDB") != NULL, "adapter backend identity");
    free(output);
    app(MODERN, "seed", original, 0);
    app(MODERN, "seed", native, 1);
    app(MODERN, "check", native, 1);
    app(MODERN, "update", native, 1);
    app(MODERN, "check-updated", native, 1);
    require_same_file(suite.app[MODERN], suite.saved_app[MODERN]);
    free(original);
    free(native);
    passed("same executable switches libraries without source or link changes");
}

static void test_explicit_migration(void) {
    char *source = path_join(suite.work, "source-basic");
    char *destination = path_join(suite.work, "migrated-basic");
    make_directory(source);
    app(MODERN, "seed", source, 0);
    char *data = path_join(source, "data.mdb");
    char *saved = snapshot(data, "basic-before");
    char *output;
    int status = migrate(source, destination, NULL, "POSIXLY_CORRECT=1", &output);
    require(status == 0, "migration accepts options after positional paths: %s", output);
    free(output);
    char *metadata = path_join(destination, "LMDB_MIGRATION.json");
    require_metadata_number(metadata, "databases", 3);
    require_metadata_number(metadata, "records", 9);
    app(MODERN, "check", destination, 1);
    app(MODERN, "update", destination, 1);
    app(MODERN, "check-updated", destination, 1);
    require_same_file(data, saved);
    app(MODERN, "check", source, 0);
    free(source); free(destination); free(data); free(saved); free(metadata);
    passed("explicit migration preserves binary main, empty and duplicate values");
}

static void test_flags_and_legacy(void) {
    for (enum abi abi = MODERN; abi <= LEGACY; ++abi) {
        char *source = path_join(suite.work, abi == MODERN ? "flags-modern" : "flags-legacy");
        char *destination = path_join(suite.work, abi == MODERN ? "flags-modern-native" : "flags-legacy-native");
        make_directory(source);
        app(abi, "seed-flags", source, 0);
        import(source, destination);
        app(abi, "check-flags", destination, 1);
        app(abi, "check-flags", source, 0);
        char *marker = path_join(destination, "LMDB-ABI");
        require_text(marker, abi == MODERN ? "1.0\n" : "0.9\n");
        free(source); free(destination); free(marker);
    }
    passed("both LMDB formats retain integer, reverse and fixed duplicate flags");
}

static void test_existing_destination(void) {
    char *source = path_join(suite.work, "source-existing");
    char *destination = path_join(suite.work, "existing-destination");
    make_directory(source);
    make_directory(destination);
    char *marker = path_join(destination, "keep");
    write_file(marker, "preserve", 8);
    app(MODERN, "seed", source, 0);
    char *output;
    require(migrate(source, destination, NULL, NULL, &output) != 0, "existing destination must fail");
    free(output);
    require_text(marker, "preserve");
    DIR *directory = opendir(destination);
    require(directory != NULL, "open protected destination");
    struct dirent *entry;
    unsigned entries = 0;
    while ((entry = readdir(directory)) != NULL) {
        if (strcmp(entry->d_name, ".") && strcmp(entry->d_name, "..")) ++entries;
    }
    require(closedir(directory) == 0 && entries == 1, "existing destination changed");
    char *empty = path_join(suite.work, "existing-empty");
    make_directory(empty);
    require(migrate(source, empty, NULL, NULL, &output) != 0, "empty destination must fail");
    free(output);
    char *link = path_join(suite.work, "existing-symlink");
    require(symlink("missing-target", link) == 0, "create destination symlink");
    require(migrate(source, link, NULL, NULL, &output) != 0, "symlink destination must fail");
    free(output);
    struct stat status;
    require(lstat(link, &status) == 0 && S_ISLNK(status.st_mode), "destination symlink replaced");
    free(source); free(destination); free(marker); free(empty); free(link);
    passed("existing, empty and symlink destinations remain untouched");
}

static void test_missing_native_marker(void) {
    char *source = path_join(suite.work, "missing-native-sidecar.mdb");
    app(LEGACY, "seed-file", source, 1);
    char *saved = snapshot(source, "marker-before");
    char *native = NULL;
    require(asprintf(&native, "%s.ulmdb", source) >= 0, "allocate sidecar path");
    remove_tree(native);
    char *output;
    require(application(LEGACY, "check-file", source, 1, NULL, &output) != 0, "missing sidecar must fail");
    require(strstr(output, "MDB_INVALID") != NULL || strstr(output, "-30793") != NULL,
            "missing sidecar must return MDB_INVALID: %s", output);
    free(output);
    require_same_file(source, saved);
    require(!path_exists(native), "missing native sidecar recreated");
    free(source); free(saved); free(native);
    passed("adapter marker never recreates a missing native sidecar");
}

static void test_single_file(void) {
    char *source = path_join(suite.work, "explicit-source.mdb");
    char *destination = path_join(suite.work, "explicit-file-native");
    app(MODERN, "seed-file", source, 0);
    char *saved = snapshot(source, "single-file-before");
    import(source, destination);
    char *metadata = path_join(destination, "LMDB_MIGRATION.json");
    require_metadata_number(metadata, "records", 9);
    app(MODERN, "check", destination, 1);
    app(MODERN, "check-file", source, 0);
    require_same_file(source, saved);
    free(source); free(destination); free(saved); free(metadata);
    passed("single-file LMDB sources are detected and preserved");
}

static void test_invalid_source(void) {
    char *source = path_join(suite.work, "not-lmdb");
    char *destination = path_join(suite.work, "failed-import");
    make_directory(source);
    char *output;
    require(migrate(source, destination, NULL, NULL, &output) != 0, "invalid source must fail");
    free(output);
    require(!path_exists(destination), "invalid source published destination");
    require_no_stage(destination);
    free(source); free(destination);
    passed("failed source export publishes nothing and cleans its stage");
}

static void test_legacy_binary(void) {
    char *native = path_join(suite.work, "legacy-native");
    make_directory(native);
    app(LEGACY, "seed-flags", native, 1);
    app(LEGACY, "check-flags", native, 1);
    require_same_file(suite.app[LEGACY], suite.saved_app[LEGACY]);
    free(native);
    passed("unchanged LMDB 0.9 executable runs directly on native storage");
}

static void test_automatic_directories(void) {
    for (enum abi abi = MODERN; abi <= LEGACY; ++abi) {
        char *source = path_join(suite.work, abi == MODERN ? "auto-dir-modern" : "auto-dir-legacy");
        make_directory(source);
        app(abi, "seed-flags", source, 0);
        char *data = path_join(source, "data.mdb");
        char *saved = snapshot(data, abi == MODERN ? "auto-dir-modern-before" : "auto-dir-legacy-before");
        app(abi, "check-flags", source, 1);
        char *current = path_join(source, ".ulmdb/CURRENT");
        char *marker = path_join(source, ".ulmdb/LMDB-ABI");
        require(path_exists(current), "automatic directory import missing CURRENT");
        require_text(marker, abi == MODERN ? "1.0\n" : "0.9\n");
        app(abi, "update", source, 1);
        app(abi, "check-updated", source, 1);
        require_same_file(data, saved);
        app(abi, "check-flags", source, 0);
        free(source); free(data); free(saved); free(current); free(marker);
    }
    passed("automatic directory imports preserve paths and original snapshots");
}

static void test_automatic_files(void) {
    for (enum abi abi = MODERN; abi <= LEGACY; ++abi) {
        char *source = path_join(suite.work, abi == MODERN ? "auto-modern.mdb" : "auto-legacy.mdb");
        app(abi, "seed-file", source, 0);
        char *saved = snapshot(source, abi == MODERN ? "auto-file-modern-before" : "auto-file-legacy-before");
        app(abi, "check-file", source, 1);
        char *current = NULL;
        require(asprintf(&current, "%s.ulmdb/CURRENT", source) >= 0, "allocate single-file CURRENT");
        require(path_exists(current), "automatic single-file import missing CURRENT");
        require_same_file(source, saved);
        app(abi, "check-file", source, 0);
        free(source); free(saved); free(current);
    }
    passed("automatic NOSUBDIR imports preserve both LMDB formats");
}

static void test_short_and_empty_files(void) {
    const unsigned char zeroes[31] = {0};
    const void *contents[] = {"x", "badlmdb", zeroes};
    const size_t lengths[] = {1, 7, 31};
    for (unsigned i = 0; i < 3; ++i) {
        for (unsigned adapter = 0; adapter < 2; ++adapter) {
            char label[64];
            require(snprintf(label, sizeof label, "short-%u-%u.mdb", i, adapter) > 0, "format short fixture");
            char *source = path_join(suite.work, label);
            write_file(source, contents[i], lengths[i]);
            char *output;
            require(application(LEGACY, "seed-file", source, (int)adapter, NULL, &output) != 0,
                    "short nonempty file must fail");
            require(strstr(output, "-30793") != NULL, "short file must return MDB_INVALID: %s", output);
            free(output);
            size_t length;
            char *after = read_file(source, &length);
            require(length == lengths[i] && !memcmp(after, contents[i], length), "short source changed");
            char *native = NULL;
            require(asprintf(&native, "%s.ulmdb", source) >= 0, "allocate short sidecar");
            require(!path_exists(native), "short file created native sidecar");
            free(source); free(after); free(native);
        }
    }
    passed("truncated nonempty LMDB files fail without sidecars");
    for (unsigned adapter = 0; adapter < 2; ++adapter) {
        char *source = path_join(suite.work, adapter ? "empty-adapter.mdb" : "empty-reference.mdb");
        write_file(source, "", 0);
        app(LEGACY, "seed-file", source, (int)adapter);
        app(LEGACY, "check-file", source, (int)adapter);
        free(source);
    }
    passed("empty single files initialize with reference and adapter libraries");
}

static void test_without_interpreter(void) {
    char *source = path_join(suite.work, "no-interpreter");
    char *empty_path = path_join(suite.work, "empty-path");
    make_directory(source);
    make_directory(empty_path);
    app(LEGACY, "seed-flags", source, 0);
    char *path_setting = formatted("PATH", empty_path);
    char *output;
    int status = application(LEGACY, "check-flags", source, 1, path_setting, &output);
    require(status == 0, "launcher and automatic migration need no interpreter: %s", output);
    free(output);
    char *current = path_join(source, ".ulmdb/CURRENT");
    require(path_exists(current), "no-interpreter run did not import data");
    free(source); free(empty_path); free(path_setting); free(current);
    passed("launcher and automatic conversion run with PATH containing no interpreters");
}

static void test_launcher_usage(void) {
    const char *arguments[] = {suite.launcher, NULL};
    char *output;
    require(run_command(arguments, NULL, &output) == 2, "launcher usage exits 2");
    require(strstr(output, "usage: convert-lmdb-to-ulmdb APPLICATION") != NULL, "renamed launcher usage");
    free(output);
    char *missing = path_join(suite.work, "missing-application");
    const char *missing_arguments[] = {suite.launcher, missing, NULL};
    require(run_command(missing_arguments, NULL, &output) == 127, "missing application exits 127");
    free(output); free(missing);
    passed("native launcher reports usage and missing application status");
}

static void output_bytes(const void *data, size_t length) {
    require(fwrite(data, 1, length, stdout) == length, "emit fake exporter bytes");
}

static void output_u64(uint64_t value, unsigned count) {
    unsigned char bytes[8];
    for (unsigned i = 0; i < count; ++i) bytes[i] = (unsigned char)(value >> (8 * i));
    output_bytes(bytes, count);
}

/* The runner doubles as an isolated malicious exporter for protocol tests. */
static int fake_exporter(const char *scenario) {
    const unsigned char magic[16] = {'U','L','M','D','B','-','M','I','G','R','A','T','E',0,2,0};
    if (!strcmp(scenario, "bad-magic")) { output_bytes("invalid", 7); return 0; }
    output_bytes(magic, sizeof magic);
    if (!strcmp(scenario, "short-header")) return 0;
    output_u64(3, 8);
    output_u64(!strcmp(scenario, "zero-readers") ? 0 : 126, 4);
    output_u64(!strcmp(scenario, "zero-mapsize") ? 0 : 64 * 1024 * 1024, 8);
    if (!strcmp(scenario, "unknown-tag")) { output_bytes("X", 1); return 0; }
    if (!strcmp(scenario, "outside-record")) { output_bytes("R", 1); return 0; }
    if (!strcmp(scenario, "unexpected-end")) { output_bytes("E", 1); return 0; }
    output_bytes("D", 1);
    output_u64(!strcmp(scenario, "unknown-flags") ? UINT32_C(0x80000000) : 0, 4);
    output_u64(!strcmp(scenario, "long-name") ? 512 : !strcmp(scenario, "nul-name") ? 2 : 0, 4);
    if (!strcmp(scenario, "long-name")) return 0;
    if (!strcmp(scenario, "nul-name")) { output_bytes("x\0", 2); return 0; }
    if (!strcmp(scenario, "nested-database")) { output_bytes("D", 1); return 0; }
    if (!strcmp(scenario, "unclosed-database")) { output_bytes("Z", 1); return 0; }
    if (!strcmp(scenario, "empty-key") || !strcmp(scenario, "long-key") ||
        !strcmp(scenario, "long-value") || !strcmp(scenario, "short-record")) {
        output_bytes("R", 1);
        output_u64(!strcmp(scenario, "empty-key") ? 0 : !strcmp(scenario, "long-key") ? 512 : 1, 8);
        output_u64(!strcmp(scenario, "long-value") ? UINT64_C(67108865) : 1, 8);
        if (!strcmp(scenario, "short-record")) output_bytes("k", 1);
        return 0;
    }
    output_bytes("EZ", 2);
    if (!strcmp(scenario, "trailing-data")) output_bytes("x", 1);
    return !strcmp(scenario, "failed-exit") ? 7 : 0;
}

static void test_malformed_exports(void) {
    const char *scenarios[] = {
        "bad-magic", "short-header", "zero-readers", "zero-mapsize", "unknown-tag",
        "outside-record", "unexpected-end", "unknown-flags", "long-name", "nul-name",
        "nested-database", "unclosed-database", "empty-key", "long-key", "long-value",
        "short-record", "trailing-data", "failed-exit"
    };
    char *source = path_join(suite.work, "malformed-export-source");
    make_directory(source);
    app(MODERN, "seed", source, 0);
    char *data = path_join(source, "data.mdb");
    char *saved = snapshot(data, "malformed-export-before");
    for (size_t i = 0; i < sizeof scenarios / sizeof *scenarios; ++i) {
        char *destination = path_join(suite.work, scenarios[i]);
        char *setting = formatted("ULMDB_FAKE_EXPORT_CASE", scenarios[i]);
        char *output;
        int status = migrate(source, destination, suite.self, setting, &output);
        require(status != 0, "malformed export %s was accepted: %s", scenarios[i], output);
        require(!path_exists(destination), "malformed export %s published destination", scenarios[i]);
        require_no_stage(destination);
        free(destination); free(setting); free(output);
    }
    require_same_file(data, saved);
    free(source); free(data); free(saved);
    passed("18 malformed or failed exporter streams publish nothing and clean stages");
}

static char *normalize_source(const char *source) {
    char *nested = path_join(source, "libraries/liblmdb");
    if (path_exists(nested)) return nested;
    free(nested);
    return absolute_path(source);
}

static void compile_application(enum abi abi) {
    const char *label = abi == MODERN ? "modern" : "legacy";
    char name[64];
    require(snprintf(name, sizeof name, "upstream-%s", label) > 0, "format upstream directory");
    suite.upstream[abi] = path_join(suite.work, name);
    make_directory(suite.upstream[abi]);
    char *library = path_join(suite.upstream[abi], "liblmdb.so.0");
    char *mdb = path_join(suite.source[abi], "mdb.c");
    char *midl = path_join(suite.source[abi], "midl.c");
    const char *library_arguments[] = {
        "-std=c11", "-O2", "-fPIC", "-shared", "-pthread", "-D_GNU_SOURCE",
        "-Wl,-soname,liblmdb.so.0", "-I", suite.source[abi], mdb, midl, "-o", library, NULL
    };
    compile_command(library_arguments);
    char *alias = path_join(suite.upstream[abi], "liblmdb.so");
    require(symlink("liblmdb.so.0", alias) == 0, "create reference library alias");
    require(snprintf(name, sizeof name, "existing-app-%s", label) > 0, "format application filename");
    suite.app[abi] = path_join(suite.work, name);
    char *app_source = path_join(suite.root, "clients/lmdb-ulmdb/tests/existing_app.c");
    const char *app_arguments[] = {
        "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-I", suite.source[abi],
        app_source, "-L", suite.upstream[abi], "-llmdb", "-o", suite.app[abi], NULL
    };
    compile_command(app_arguments);
    require(snprintf(name, sizeof name, "saved-app-%s", label) > 0, "format saved application filename");
    suite.saved_app[abi] = snapshot(suite.app[abi], name);
    free(library); free(mdb); free(midl); free(alias); free(app_source);
}

static void usage(void) {
    puts("usage: ulmdb-lmdb-tests [--no-build] [--package PATH] [--lmdb-source PATH] [--legacy-source PATH]");
}

int main(int argc, char **argv) {
    const char *fake = getenv("ULMDB_FAKE_EXPORT_CASE");
    if (fake) return fake_exporter(fake);
    char executable[PATH_MAX];
    ssize_t count = readlink("/proc/self/exe", executable, sizeof executable - 1);
    require(count > 0 && (size_t)count < sizeof executable - 1, "locate native test executable");
    executable[count] = '\0';
    suite.self = strdup(executable);
    char *slash = strrchr(executable, '/');
    require(slash != NULL, "find test binary directory");
    *slash = '\0';
    slash = strrchr(executable, '/');
    require(slash != NULL, "find repository directory");
    *slash = '\0';
    suite.root = strdup(executable);
    require(suite.self != NULL && suite.root != NULL, "allocate executable paths");
    const char *home = getenv("HOME");
    require(home != NULL, "HOME is needed for default cached source paths");
    const char *modern_env = getenv("ULMDB_LMDB_SOURCE");
    char *modern_default = modern_env ? absolute_path(modern_env) : path_join(suite.root, "benchmark/support/lmdb/vendor");
    char *legacy_default = path_join(home, ".cache/ulmdb-compat/lmdb-0.9.31");
    suite.package = path_join(suite.root, "target/lmdb-ulmdb");
    suite.source[MODERN] = normalize_source(modern_default);
    suite.source[LEGACY] = normalize_source(legacy_default);
    free(modern_default); free(legacy_default);
    int no_build = 0;
    for (int i = 1; i < argc; ++i) {
        if (!strcmp(argv[i], "--no-build")) no_build = 1;
        else if (!strcmp(argv[i], "-v")) { /* Output is always case-oriented. */ }
        else if (!strcmp(argv[i], "--help")) { usage(); cleanup(); return 0; }
        else if (!strcmp(argv[i], "--package") || !strcmp(argv[i], "--lmdb-source") || !strcmp(argv[i], "--legacy-source")) {
            const char *option = argv[i];
            require(++i < argc, "missing value for %s", option);
            char **target = !strcmp(option, "--package") ? &suite.package :
                !strcmp(option, "--lmdb-source") ? &suite.source[MODERN] : &suite.source[LEGACY];
            free(*target);
            *target = !strcmp(option, "--package") ? absolute_path(argv[i]) : normalize_source(argv[i]);
        } else fail("unknown argument: %s", argv[i]);
    }
    if (!no_build) {
        char *directory = path_join(suite.root, "clients/lmdb-ulmdb");
        char *output = formatted("OUTPUT", suite.package);
        const char *arguments[] = {"make", "-C", directory, "package", output, NULL};
        checked_command(arguments, NULL);
        free(directory); free(output);
    }
    /* Reference builds always retain the source pin checks, including when the
       package was built separately and this runner uses --no-build. */
    struct build_source verified;
    build_modern_source(suite.root, suite.source[MODERN], &verified);
    char *adapter_source = path_join(suite.root, "clients/lmdb-ulmdb");
    build_legacy_source(adapter_source, suite.source[LEGACY], 0, &verified);
    free(adapter_source);
    suite.launcher = path_join(suite.package, "bin/convert-lmdb-to-ulmdb");
    suite.migrator = path_join(suite.package, "bin/ulmdb-migrate");
    suite.library = path_join(suite.package, "lib/liblmdb.so.0");
    suite.exporter = path_join(suite.package, "bin/lmdb-export");
    strcpy(suite.work, "/tmp/ulmdb-native-adapter-tests-XXXXXX");
    require(mkdtemp(suite.work) != NULL, "create test directory: %s", strerror(errno));
    require(atexit(cleanup) == 0, "register test cleanup");
    compile_application(MODERN);
    compile_application(LEGACY);
    test_launcher_usage();
    test_same_binary();
    test_explicit_migration();
    test_flags_and_legacy();
    test_existing_destination();
    test_missing_native_marker();
    test_single_file();
    test_invalid_source();
    test_legacy_binary();
    test_automatic_directories();
    test_automatic_files();
    test_short_and_empty_files();
    test_without_interpreter();
    test_malformed_exports();
    require_same_file(suite.app[MODERN], suite.saved_app[MODERN]);
    require_same_file(suite.app[LEGACY], suite.saved_app[LEGACY]);
    printf("PASS: %u native adapter integration cases\n", suite.passed);
    return 0;
}
