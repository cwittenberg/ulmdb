/* Differential evidence uses public source pins and unchanged application code. */
#include "support.h"
#include "parity_report.h"
#include <ctype.h>
#include <dlfcn.h>
#include <errno.h>
#include <inttypes.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <wordexp.h>

enum abi { LEGACY, MODERN, CURRENT09, ABI_COUNT };
struct fixture { const char *name; const char *sources[4]; int randomized; };
static const struct fixture fixtures[] = {
    {"Common API", {"clients/c/tests/compatibility.c", NULL}, 0},
    {"Ordering and duplicate flags", {"clients/c/tests/features.c", NULL}, 0},
    {"Reserve buffer lifetimes", {"clients/c/tests/reserve.c", NULL}, 0},
    {"Cursor deletion positioning", {"clients/c/tests/cursor_delete.c", NULL}, 0},
    {"Database clear and drop", {"clients/c/tests/drop.c", NULL}, 0},
    {"Environment and transaction contract", {"clients/lmdb-ulmdb/tests/parity_contract.c", NULL}, 0},
    {"Cursor input and write contract", {"clients/lmdb-ulmdb/tests/parity_cursor.c", NULL}, 0},
    {"Randomized KV and duplicates", {"clients/lmdb-ulmdb/tests/parity_workload.c", "clients/lmdb-ulmdb/tests/parity_workload_data.c", NULL}, 1}
};
static const uint64_t seeds[] = {1, 42, 314159265};
static struct parity_report report;
static char *root, *package, *upstream[ABI_COUNT], *launcher;
static char work[PATH_MAX];
static uint64_t operations = 4096;
static int report_written, keep_traces, feature_profile, fetch_current09;

static const char *abi_name(enum abi abi) {
    static const char *names[] = {"0.9.31", "1.0.2", "0.9.36"};
    return names[abi];
}
static const char *abi_mode(enum abi abi) { return abi == MODERN ? "1.0" : abi == CURRENT09 ? "0.9.36" : "0.9.31"; }
static void cleanup(void) {
    if (work[0]) remove_tree(work);
    free(root); free(package); free(launcher);
    for (unsigned i = 0; i < ABI_COUNT; ++i) free(upstream[i]);
    free((char *)report.compiler); free((char *)report.directory);
}
static void evidence_on_failure(void) {
    if (!report_written && report.directory) (void)parity_report_write(&report);
}
static struct parity_result *new_result(const char *name, const char *abi) {
    require(report.count < PARITY_MAX_RESULTS, "too many conformance results");
    struct parity_result *result = report.results + report.count++;
    memset(result, 0, sizeof *result);
    result->name = name; result->abi = abi;
    result->reference_status = result->adapter_status = -1;
    return result;
}
static char *setting(const char *name, const char *value) { return build_format("%s=%s", name, value); }
static void hash_file(const char *path, char digest[65]) {
    uint64_t ignored;
    build_hash(path, &ignored, digest);
}
static void hash_trace(const char *label, const char *output, char digest[65]) {
    char *path = path_join(work, label);
    write_file(path, output, strlen(output)); hash_file(path, digest);
    require(unlink(path) == 0, "remove temporary trace: %s", strerror(errno));
    free(path);
}
static void save_traces(size_t index, const struct parity_result *result,
                         const char *reference, const char *adapter) {
    if (result->passed && !keep_traces) return;
    char *directory = path_join(report.directory, result->passed ? "traces" : "failures");
    build_mkdirs(directory);
    char *left = build_format("%s/%02zu-reference.txt", directory, index);
    char *right = build_format("%s/%02zu-adapter.txt", directory, index);
    write_file(left, reference, strlen(reference)); write_file(right, adapter, strlen(adapter));
    free(left); free(right); free(directory);
}
static void record_trace(struct parity_result *result, const char *executable,
                          const char *candidate, const char *reference, const char *adapter) {
    hash_file(executable, result->executable_sha256);
    hash_file(candidate, result->adapter_executable_sha256);
    hash_trace("reference-trace", reference, result->reference_sha256);
    hash_trace("adapter-trace", adapter, result->adapter_sha256);
    result->trace_bytes = strlen(reference);
    result->passed = !result->reference_status && !result->adapter_status && !strcmp(reference, adapter);
    save_traces((size_t)(result - report.results), result, reference, adapter);
    printf("%s: %s / LMDB %s", result->passed ? "PASS" : "FAIL", result->name, result->abi);
    if (result->operations) printf(" / seed %" PRIu64 " / %" PRIu64 " operations", result->seed, result->operations);
    putchar('\n'); fflush(stdout);
    if (!result->passed) {
        size_t offset = 0;
        while (reference[offset] && reference[offset] == adapter[offset]) ++offset;
        fprintf(stderr, "Trace mismatch or failed process at byte %zu (reference=%d, adapter=%d); "
                "evidence in %s/failures\n", offset, result->reference_status, result->adapter_status, report.directory);
    }
}
static char *compiler_identity(void) {
    const char *compiler = getenv("CC");
    wordexp_t words;
    int parsed = wordexp(compiler && *compiler ? compiler : "cc", &words, WRDE_NOCMD);
    require(!parsed && words.we_wordc, "parse compiler command");
    const char **arguments = calloc(words.we_wordc + 2, sizeof *arguments);
    require(arguments != NULL, "allocate compiler command");
    for (size_t i = 0; i < words.we_wordc; ++i) arguments[i] = words.we_wordv[i];
    arguments[words.we_wordc] = "--version";
    char *output;
    int status = run_command(arguments, NULL, &output);
    require(status == 0, "compiler identification failed: %s", output);
    char *line = strchr(output, '\n'); if (line) *line = 0;
    char *identity = build_format("%s; command: %s", output, compiler && *compiler ? compiler : "cc");
    free(output); free(arguments); wordfree(&words);
    return identity;
}
static void build_reference(enum abi abi) {
    upstream[abi] = build_format("%s/reference-%s", work, abi_name(abi));
    make_directory(upstream[abi]);
    char *library = path_join(upstream[abi], "liblmdb.so.0");
    char *mdb = path_join(report.sources[abi].directory, "mdb.c");
    char *midl = path_join(report.sources[abi].directory, "midl.c");
    const char *arguments[24] = {"-std=c11", "-O2", "-fPIC", "-shared", "-pthread", "-D_GNU_SOURCE",
        "-Wl,-soname,liblmdb.so.0", "-I", report.sources[abi].directory, mdb, midl, NULL};
    size_t count = 0;
    while (arguments[count]) ++count;
    char *module = NULL;
    if (abi == MODERN) {
        module = path_join(report.sources[abi].directory, "module.c");
        arguments[count++] = module;
        arguments[count++] = "-ldl";
    }
    arguments[count++] = "-o";
    arguments[count++] = library;
    arguments[count] = NULL;
    compile_command(arguments);
    free(module);
    char *alias = path_join(upstream[abi], "liblmdb.so");
    require(!symlink("liblmdb.so.0", alias), "create reference link: %s", strerror(errno));
    free(library); free(mdb); free(midl); free(alias);
}
static char *compile_fixture(const struct fixture *fixture, size_t index, enum abi abi, int own_header) {
    char *executable = build_format("%s/fixture-%zu-%u-%d", work, index, (unsigned)abi, own_header);
    char *own_include = path_join(package, "include");
    char *own_library = path_join(package, "lib");
    const char *arguments[32] = {"-std=c11", "-O2", "-D_GNU_SOURCE", "-Wall", "-Wextra", "-Werror", "-pthread", "-I", NULL};
    arguments[8] = own_header ? own_include : report.sources[abi].directory;
    size_t count = 9, first_source = count;
    for (size_t i = 0; fixture->sources[i]; ++i) arguments[count++] = path_join(root, fixture->sources[i]);
    size_t source_end = count;
    arguments[count++] = "-L"; arguments[count++] = own_header ? own_library : upstream[abi];
    arguments[count++] = "-llmdb"; arguments[count++] = "-o"; arguments[count++] = executable; arguments[count] = NULL;
    compile_command(arguments);
    for (size_t i = first_source; i < source_end; ++i) free((char *)arguments[i]);
    free(own_include); free(own_library);
    return executable;
}
static int execute_fixture(const char *executable, const char *directory, enum abi abi,
                            int adapter, uint64_t seed, int randomized, char **output) {
    const char *arguments[8];
    size_t count = 0;
    char seed_text[32], operation_text[32];
    if (adapter) arguments[count++] = launcher;
    arguments[count++] = executable; arguments[count++] = directory;
    if (randomized) {
        snprintf(seed_text, sizeof seed_text, "%" PRIu64, seed);
        snprintf(operation_text, sizeof operation_text, "%" PRIu64, operations);
        arguments[count++] = seed_text; arguments[count++] = operation_text;
    }
    arguments[count] = NULL;
    char *library = setting("LD_LIBRARY_PATH", upstream[abi]);
    char *mode = setting("ULMDB_LMDB_ABI", abi_mode(abi));
    const char *environment[] = {library, mode, "ULMDB_MIGRATOR", "ULMDB_LIBRARY", "LD_PRELOAD", NULL};
    int status = run_command(arguments, environment, output);
    free(library); free(mode);
    return status;
}
static void run_fixture(const struct fixture *fixture, size_t index, enum abi abi) {
    char *executable = compile_fixture(fixture, index, abi, 0);
    size_t repetitions = fixture->randomized ? sizeof seeds / sizeof *seeds : 1;
    for (size_t repeat = 0; repeat < repetitions; ++repeat) {
        struct parity_result *result = new_result(fixture->name, abi_name(abi));
        result->seed = fixture->randomized ? seeds[repeat] : 0;
        result->operations = fixture->randomized ? operations : 0;
        char *left_directory = build_format("%s/case-%zu-%u-%zu-reference", work, index, (unsigned)abi, repeat);
        char *right_directory = build_format("%s/case-%zu-%u-%zu-adapter", work, index, (unsigned)abi, repeat);
        make_directory(left_directory); make_directory(right_directory);
        char before[65], after[65]; hash_file(executable, before);
        char *reference, *adapter;
        result->reference_status = execute_fixture(executable, left_directory, abi, 0, result->seed, fixture->randomized, &reference);
        result->adapter_status = execute_fixture(executable, right_directory, abi, 1, result->seed, fixture->randomized, &adapter);
        hash_file(executable, after); require(!strcmp(before, after), "application binary changed during loader swap");
        record_trace(result, executable, executable, reference, adapter);
        free(reference); free(adapter); free(left_directory); free(right_directory);
    }
    free(executable);
}
/* The module fixture takes an absolute plugin path rather than a data root. */
static void run_module_fixture(size_t index, int lifetime) {
    const struct fixture fixture = {
        lifetime ? "Loaded crypto callbacks and final module release" : "Crypto module default/custom hooks and unload",
        {lifetime ? "clients/lmdb-ulmdb/tests/parity_crypto_lifetime.c" : "clients/c/tests/crypto_module.c", NULL}, 0};
    char *plugin = build_format("%s/crypto-module-%zu.so", work, index);
    char *source = path_join(root, lifetime ? "clients/lmdb-ulmdb/tests/parity_crypto_lifetime_plugin.c" : "clients/c/tests/crypto_module_plugin.c");
    const char *arguments[] = {"-std=c11", "-O2", "-fPIC", "-shared", "-Wall", "-Wextra", "-Werror",
        "-I", report.sources[MODERN].directory, source, "-o", plugin, NULL};
    compile_command(arguments);
    char *executable = compile_fixture(&fixture, index, MODERN, 0);
    char binary_before[65], binary_after[65], plugin_before[65], plugin_after[65];
    hash_file(executable, binary_before); hash_file(plugin, plugin_before);
    char *left_argument, *right_argument, *left_plugin = NULL, *right_plugin = NULL;
    if (lifetime) {
        left_argument = build_format("%s/module-%zu-reference", work, index);
        right_argument = build_format("%s/module-%zu-adapter", work, index);
        make_directory(left_argument); make_directory(right_argument);
        left_plugin = path_join(left_argument, "crypto-module.so");
        right_plugin = path_join(right_argument, "crypto-module.so");
        build_copy(plugin, left_plugin, 0500); build_copy(plugin, right_plugin, 0500);
        hash_file(left_plugin, plugin_after);
        require(!strcmp(plugin_before, plugin_after), "reference plugin copy differs");
        hash_file(right_plugin, plugin_after);
        require(!strcmp(plugin_before, plugin_after), "adapter plugin copy differs");
    } else {
        left_argument = build_format("%s", plugin);
        right_argument = build_format("%s", plugin);
    }
    struct parity_result *result = new_result(fixture.name, abi_name(MODERN));
    char *reference, *adapter;
    result->reference_status = execute_fixture(executable, left_argument, MODERN, 0, 0, 0, &reference);
    result->adapter_status = execute_fixture(executable, right_argument, MODERN, 1, 0, 0, &adapter);
    hash_file(executable, binary_after); hash_file(plugin, plugin_after);
    require(!strcmp(binary_before, binary_after) && !strcmp(plugin_before, plugin_after),
            "module application or plugin changed during loader swap");
    if (lifetime) {
        hash_file(left_plugin, plugin_after);
        require(!strcmp(plugin_before, plugin_after), "reference plugin changed");
        hash_file(right_plugin, plugin_after);
        require(!strcmp(plugin_before, plugin_after), "adapter plugin changed");
    }
    /* Bind the identical auxiliary plugin bytes in both retained traces. */
    char *left = build_format("module-sha256=%s\n%s", plugin_before, reference);
    char *right = build_format("module-sha256=%s\n%s", plugin_before, adapter);
    record_trace(result, executable, executable, left, right);
    free(left); free(right); free(reference); free(adapter);
    free(left_argument); free(right_argument); free(left_plugin); free(right_plugin);
    free(executable); free(plugin); free(source);
}
static void run_abi_probe(const struct fixture *fixture, enum abi first, unsigned end, size_t index) {
    char *candidate = compile_fixture(fixture, index, first, 1);
    for (enum abi abi = first; (unsigned)abi < end; ++abi) {
        struct parity_result *result = new_result(fixture->name, abi_name(abi));
        char *reference_executable = compile_fixture(fixture, index + 1, abi, 0);
        char *left, *right;
        result->reference_status = execute_fixture(reference_executable, work, abi, 0, 0, 0, &left);
        result->adapter_status = execute_fixture(candidate, work, abi, 1, 0, 0, &right);
        record_trace(result, reference_executable, candidate, left, right);
        free(reference_executable); free(left); free(right);
    }
    free(candidate);
}
static void run_header_probe(void) {
    const struct fixture fixture = {"Header constants and ABI layouts", {"clients/lmdb-ulmdb/tests/parity_abi.c", NULL}, 0};
    run_abi_probe(&fixture, LEGACY, report.reference_count, 98);
}
static int symbol_index(const struct parity_symbols *symbols, const char *name) {
    for (unsigned i = 0; i < symbols->count; ++i)
        if (!strcmp(symbols->names[i], name)) return (int)i;
    return -1;
}
static void header_symbols(const char *path, struct parity_symbols *symbols) {
    size_t length;
    char *source = read_file(path, &length);
    /* Erase comments and preprocessor directives while preserving token spacing. */
    int comment = 0, directive = 0, beginning = 1;
    for (size_t i = 0; i < length; ++i) {
        char current = source[i], next = i + 1 < length ? source[i + 1] : 0;
        if (comment) {
            source[i] = ' ';
            if (comment == 1 && current == '*' && next == '/') { source[++i] = ' '; comment = 0; }
            else if (comment == 2 && current == '\n') { comment = 0; beginning = 1; }
            continue;
        }
        if (directive) { source[i] = ' '; if (current == '\n') { directive = 0; beginning = 1; } continue; }
        if (current == '/' && (next == '*' || next == '/')) {
            comment = next == '*' ? 1 : 2;
            source[i] = ' '; ++i; source[i] = ' '; continue;
        }
        if (beginning && current == '#') { directive = 1; source[i] = ' '; continue; }
        if (current == '\n') beginning = 1;
        else if (!isspace((unsigned char)current)) beginning = 0;
    }
    memset(symbols, 0, sizeof *symbols);
    for (char *p = source; (p = strstr(p, "mdb_")); ) {
        if (p > source && (isalnum((unsigned char)p[-1]) || p[-1] == '_')) { p += 4; continue; }
        char *end = p + 4;
        while (isalnum((unsigned char)*end) || *end == '_') ++end;
        char *next = end; while (isspace((unsigned char)*next)) ++next;
        if (*next != '(') { p = end; continue; }
        size_t size = (size_t)(end - p);
        require(size < sizeof symbols->names[0], "public symbol exceeds name bound");
        char name[96]; memcpy(name, p, size); name[size] = 0;
        if (symbol_index(symbols, name) < 0) {
            require(symbols->count < PARITY_MAX_SYMBOLS, "too many public symbols");
            strcpy(symbols->names[symbols->count++], name);
        }
        p = end;
    }
    free(source);
    require(symbols->count > 40, "incomplete public-header symbol extraction");
}
static void check_symbols(void) {
    char *legacy = path_join(report.sources[LEGACY].directory, "lmdb.h");
    char *modern = path_join(report.sources[MODERN].directory, "lmdb.h");
    header_symbols(legacy, &report.common);
    struct parity_symbols modern_all;
    header_symbols(modern, &modern_all);
    char *library = path_join(package, "lib/liblmdb.so.0");
    void *handle = dlopen(library, RTLD_NOW | RTLD_LOCAL);
    require(handle != NULL, "load adapter to verify symbols: %s", dlerror());
    for (unsigned i = 0; i < report.common.count; ++i) {
        report.common.present[i] = dlsym(handle, report.common.names[i]) != NULL;
        report.common.supported += report.common.present[i];
        if (!report.common.present[i]) fprintf(stderr, "Missing common public symbol: %s\n", report.common.names[i]);
    }
    for (unsigned i = 0; i < modern_all.count; ++i) {
        if (symbol_index(&report.common, modern_all.names[i]) >= 0) continue;
        unsigned index = report.modern_extensions.count++;
        strcpy(report.modern_extensions.names[index], modern_all.names[i]);
        report.modern_extensions.present[index] = dlsym(handle, modern_all.names[i]) != NULL;
        report.modern_extensions.supported += report.modern_extensions.present[index];
    }
    int cursor_index = symbol_index(&report.modern_extensions, "mdb_cursor_is_db");
    struct parity_result *result = new_result("LMDB 1.0 catalog predicate export", "1.0.2");
    result->passed = cursor_index >= 0 && report.modern_extensions.present[cursor_index];
    result->reference_status = 0; result->adapter_status = result->passed ? 0 : 1;
    printf("%s: common symbols %u/%u; selected LMDB 1.0 catalog predicate %s\n",
           report.common.supported == report.common.count ? "PASS" : "FAIL",
           report.common.supported, report.common.count, result->passed ? "PASS" : "FAIL");
    if (feature_profile) {
        int flags_index = symbol_index(&report.modern_extensions, "mdb_txn_flags");
        result = new_result("LMDB 1.0 transaction flags export", "1.0.2");
        result->passed = flags_index >= 0 && report.modern_extensions.present[flags_index];
        result->reference_status = 0; result->adapter_status = result->passed ? 0 : 1;
        printf("%s: selected LMDB 1.0 transaction flags export\n", result->passed ? "PASS" : "FAIL");
    }
    dlclose(handle); free(legacy); free(modern); free(library);
}
static void deployment(void) {
    struct parity_result *result = new_result("Deployment and automatic migration (15 groups)", "both");
    char *runner = path_join(root, "target/lmdb-ulmdb-tests");
    require(path_exists(runner), "build the native deployment runner before parity: make -C clients/lmdb-ulmdb test");
    const char *arguments[] = {runner, "--no-build", "--package", package, "--lmdb-source",
        report.sources[MODERN].directory, "--legacy-source", report.sources[LEGACY].directory, NULL};
    const char *environment[] = {"LD_PRELOAD", "ULMDB_LIBRARY", NULL};
    char *output;
    result->adapter_status = run_command(arguments, environment, &output);
    result->reference_status = 0; result->passed = !result->adapter_status;
    hash_file(runner, result->executable_sha256);
    strcpy(result->adapter_executable_sha256, result->executable_sha256);
    hash_trace("deployment-trace", output, result->adapter_sha256);
    result->trace_bytes = strlen(output);
    save_traces(report.count - 1, result, "", output);
    printf("%s: deployment and automatic migration\n", result->passed ? "PASS" : "FAIL");
    if (!result->passed) fputs(output, stderr);
    free(output); free(runner);
}
static void usage(void) {
    puts("usage: ulmdb-lmdb-parity [--package PATH] [--report-dir PATH] [--operations COUNT]\n"
         "       [--lmdb-source PATH] [--legacy-source PATH] [--keep-traces]\n"
         "       [--profile common-v1|features-v2] [--current09-source PATH] [--fetch-current09]");
}
int main(int argc, char **argv) {
    char executable[PATH_MAX];
    ssize_t count = readlink("/proc/self/exe", executable, sizeof executable - 1);
    require(count > 0 && (size_t)count < sizeof executable - 1, "locate parity executable");
    executable[count] = 0;
    char *last = strrchr(executable, '/'); require(last != NULL, "locate parity binary directory"); *last = 0;
    last = strrchr(executable, '/'); require(last != NULL, "locate repository root"); *last = 0;
    root = strdup(executable); require(root != NULL, "allocate repository path");
    package = path_join(root, "target/lmdb-ulmdb");
    report.directory = path_join(root, "clients/lmdb-ulmdb/conformance");
    const char *home = getenv("HOME"); require(home != NULL, "HOME needed for source cache defaults");
    char *modern = getenv("ULMDB_LMDB_SOURCE") ? absolute_path(getenv("ULMDB_LMDB_SOURCE")) :
        path_join(root, "support/lmdb/vendor");
    char *legacy = path_join(home, ".cache/ulmdb-compat/lmdb-0.9.31");
    char *current09 = path_join(home, ".cache/ulmdb-compat/lmdb-0.9.36");
    int explicit_report = 0;
    report.profile = "lmdb-common-functional-v1";
    for (int i = 1; i < argc; ++i) {
        if (!strcmp(argv[i], "--help")) { usage(); free(modern); free(legacy); free(current09); cleanup(); return 0; }
        if (!strcmp(argv[i], "--keep-traces")) { keep_traces = 1; continue; }
        if (!strcmp(argv[i], "--fetch-current09")) { fetch_current09 = 1; continue; }
        const char *option = argv[i]; require(++i < argc, "missing argument to %s", option);
        if (!strcmp(option, "--profile")) {
            require(!strcmp(argv[i], "common-v1") || !strcmp(argv[i], "features-v2"), "unknown profile: %s", argv[i]);
            feature_profile = !strcmp(argv[i], "features-v2");
            continue;
        }
        if (!strcmp(option, "--operations")) {
            char *end; errno = 0; operations = strtoull(argv[i], &end, 10);
            require(!errno && *argv[i] && !*end && operations && operations <= 1000000,
                    "operation count must be 1..1000000");
            continue;
        }
        char **target = !strcmp(option, "--package") ? &package : !strcmp(option, "--report-dir") ?
            (char **)&report.directory : !strcmp(option, "--lmdb-source") ? &modern :
            !strcmp(option, "--legacy-source") ? &legacy :
            !strcmp(option, "--current09-source") ? &current09 : NULL;
        require(target != NULL, "unknown option: %s", option);
        if (!strcmp(option, "--report-dir")) explicit_report = 1;
        free(*target); *target = absolute_path(argv[i]);
    }
    report.reference_count = feature_profile ? ABI_COUNT : 2;
    if (feature_profile) {
        report.profile = "lmdb-features-v2";
        if (!explicit_report) {
            free((char *)report.directory);
            report.directory = path_join(root, "clients/lmdb-ulmdb/conformance/features");
        }
    }
    for (unsigned i = 0; i < report.reference_count; ++i) report.versions[i] = abi_name((enum abi)i);
    strcpy(work, "/tmp/ulmdb-functional-parity-XXXXXX");
    require(mkdtemp(work) != NULL, "create parity work directory: %s", strerror(errno));
    require(!atexit(cleanup), "register parity cleanup");
    build_mkdirs(report.directory);
    require(!atexit(evidence_on_failure), "register evidence writer");
    report.compiler = compiler_identity();
    char *here = path_join(root, "clients/lmdb-ulmdb");
    build_legacy_source(here, legacy, 0, report.sources + LEGACY);
    build_modern_source(root, modern, report.sources + MODERN);
    if (feature_profile) build_current09_source(here, current09, fetch_current09, report.sources + CURRENT09);
    free(here); free(legacy); free(modern); free(current09);
    char *library = path_join(package, "lib/liblmdb.so.0");
    build_hash(library, &report.adapter_bytes, report.adapter_sha256); free(library);
    launcher = path_join(package, "bin/convert-lmdb-to-ulmdb");
    check_symbols();
    for (unsigned i = 0; i < report.reference_count; ++i) build_reference((enum abi)i);
    run_header_probe();
    for (size_t i = 0; i < sizeof fixtures / sizeof *fixtures; ++i)
        for (enum abi abi = LEGACY; (unsigned)abi < report.reference_count; ++abi) run_fixture(fixtures + i, i, abi);
    if (feature_profile) {
        const struct fixture modern = {"LMDB 1.0 transaction flags", {"clients/lmdb-ulmdb/tests/parity_modern.c", NULL}, 0};
        const struct fixture modern_abi = {"Selected LMDB 1.0 declarations and ABI", {"clients/lmdb-ulmdb/tests/parity_modern_abi.c", NULL}, 0};
        run_abi_probe(&modern_abi, MODERN, MODERN + 1, 100);
        run_fixture(&modern, 102, MODERN);
        const struct fixture children = {"LMDB 1.0 read-only child transactions", {"clients/lmdb-ulmdb/tests/parity_readonly_children.c", NULL}, 0};
        run_fixture(&children, 105, MODERN);
        const struct fixture catalog = {"Version-specific main catalog and DBI lifetimes", {"clients/lmdb-ulmdb/tests/parity_catalog_lifetimes.c", NULL}, 0};
        for (enum abi abi = LEGACY; (unsigned)abi < report.reference_count; ++abi)
            run_fixture(&catalog, 106, abi);
        const struct fixture recovery = {"Transactional DBI registration and recovery", {"clients/lmdb-ulmdb/tests/parity_catalog_recovery.c", NULL}, 0};
        for (enum abi abi = LEGACY; (unsigned)abi < report.reference_count; ++abi)
            run_fixture(&recovery, 107, abi);
        const struct fixture visibility = {"Transaction-local DBI alias visibility", {"clients/lmdb-ulmdb/tests/parity_catalog_visibility.c", NULL}, 0};
        for (enum abi abi = LEGACY; (unsigned)abi < report.reference_count; ++abi)
            run_fixture(&visibility, 108, abi);
        const struct fixture comparison = {"Global comparator metadata on old DBI aliases", {"clients/lmdb-ulmdb/tests/parity_catalog_comparison.c", NULL}, 0};
        for (enum abi abi = LEGACY; (unsigned)abi < report.reference_count; ++abi)
            run_fixture(&comparison, 109, abi);
        const struct fixture first_ids = {"Absolute initial transaction IDs", {"clients/lmdb-ulmdb/tests/parity_first_ids_suite.c", NULL}, 0};
        for (enum abi abi = LEGACY; (unsigned)abi < report.reference_count; ++abi)
            run_fixture(&first_ids, 110, abi);
        const struct fixture preparation = {"LMDB 1.0 transaction preparation", {"clients/lmdb-ulmdb/tests/parity_prepare_suite.c", NULL}, 0};
        run_fixture(&preparation, 111, MODERN);
        const struct fixture prepared_metadata = {"LMDB 1.0 prepared metadata helpers", {"clients/lmdb-ulmdb/tests/parity_prepare_metadata_suite.c", NULL}, 0};
        run_fixture(&prepared_metadata, 112, MODERN);
        const struct fixture rollback = {"LMDB 1.0 last-commit rollback", {"clients/lmdb-ulmdb/tests/parity_rollback_suite.c", NULL}, 0};
        run_fixture(&rollback, 113, MODERN);
        const struct fixture main_callbacks = {"MAIN callbacks across first writer bootstrap", {"clients/lmdb-ulmdb/tests/parity_main_callback_lifetime.c", NULL}, 0};
        for (enum abi abi = LEGACY; (unsigned)abi < report.reference_count; ++abi)
            run_fixture(&main_callbacks, 114, abi);
        const struct fixture main_reopen = {"MAIN public reopen resets comparison callbacks", {"clients/lmdb-ulmdb/tests/parity_main_callback_reopen.c", NULL}, 0};
        for (enum abi abi = LEGACY; (unsigned)abi < report.reference_count; ++abi)
            run_fixture(&main_reopen, 115, abi);
        const struct fixture main_pending = {"LMDB 1.0 prepared MAIN callback metadata", {"clients/lmdb-ulmdb/tests/parity_prepare_main_callbacks.c", NULL}, 0};
        run_fixture(&main_pending, 116, MODERN);
        const struct fixture page_geometry = {"LMDB 1.0 page geometry and key limits", {"clients/lmdb-ulmdb/tests/parity_page_geometry.c", NULL}, 0};
        run_fixture(&page_geometry, 117, MODERN);
        const struct fixture ordering_index = {"Indexed key and duplicate comparisons", {"clients/lmdb-ulmdb/tests/parity_ordering_index.c", NULL}, 0};
        run_fixture(&ordering_index, 118, MODERN);
        const struct fixture reserve_native = {"Reserve pointer and nested transaction lifetimes", {"clients/lmdb-ulmdb/tests/parity_reserve_native.c", NULL}, 0};
        run_fixture(&reserve_native, 119, MODERN);
        const struct fixture incremental = {"LMDB 1.0 incremental path/FD dump and load", {"clients/lmdb-ulmdb/tests/parity_incremental_extensions.c", NULL}, 0};
        run_fixture(&incremental, 120, MODERN);
        run_module_fixture(121, 0);
        const struct fixture transforms = {"Checksum/encryption callbacks and module setup", {"clients/lmdb-ulmdb/tests/parity_transform_callbacks_shared.c", NULL}, 0};
        run_fixture(&transforms, 122, MODERN);
        run_module_fixture(123, 1);
        const struct fixture readers = {"Reader slot high-water contract", {"clients/lmdb-ulmdb/tests/parity_readers.c", NULL}, 0};
        for (enum abi abi = LEGACY; (unsigned)abi < report.reference_count; ++abi)
            run_fixture(&readers, 103, abi);
        const struct fixture copy_readers = {"Copy readonly reader-slot accounting", {"clients/lmdb-ulmdb/tests/parity_copy_readers.c", NULL}, 0};
        for (enum abi abi = LEGACY; (unsigned)abi < report.reference_count; ++abi)
            run_fixture(&copy_readers, 124, abi);
        const struct fixture current09 = {"LMDB 0.9.36 empty duplicate rejection", {"clients/lmdb-ulmdb/tests/parity_current09.c", NULL}, 0};
        run_fixture(&current09, 104, CURRENT09);
    }
    deployment();
    report.complete = 1;
    require(!parity_report_write(&report), "write conformance receipt"); report_written = 1;
    int passed = parity_report_passed(&report);
    printf("%s: %zu conformance checks; receipt and table: %s\n", passed ? "PASS" : "FAIL", report.count, report.directory);
    return passed ? 0 : 1;
}
