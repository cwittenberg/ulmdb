/* Build the LMDB ABI package; the database engine remains the Rust library. */
#include "build_support.h"

#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#ifndef ULMDB_SOURCE_ROOT
#error ULMDB_SOURCE_ROOT must identify the repository root
#endif

#if defined(__APPLE__)
#define TOOL_PLATFORM_DEFINE "-D_DARWIN_C_SOURCE"
#define TOOL_PRIVATE_LIBS "-lSystem -lc -lm"
#define TOOL_OPERATOR_RPATH "-Wl,-rpath,@loader_path/../lib"
#else
#define TOOL_PLATFORM_DEFINE "-D_GNU_SOURCE"
#define TOOL_PRIVATE_LIBS "-ldl -lpthread -lm -lrt -lutil"
#define TOOL_OPERATOR_RPATH "-Wl,-rpath,$ORIGIN/../lib"
#endif

struct options {
    const char *profile;
    const char *output;
    const char *modern;
    const char *legacy;
    int no_cargo;
    int fetch_legacy;
};

static void usage(FILE *stream) {
    fprintf(stream, "usage: ulmdb-adapter-build [--profile debug|release] [--output DIRECTORY]\n"
                    "       [--lmdb-source DIRECTORY] [--legacy-source DIRECTORY]\n"
                    "       [--no-cargo] [--fetch-legacy]\n");
}

static const char *argument(int *index, int count, char **arguments) {
    if (++*index == count) build_fail("missing value for %s", arguments[*index-1]);
    return arguments[*index];
}

static struct options parse(int count, char **arguments) {
    const char *home = getenv("HOME");
    if (!home) build_fail("HOME is required to locate the upstream source caches");
    struct options options = {"debug", NULL, getenv("ULMDB_LMDB_SOURCE"), NULL, 0, 0};
    options.output = build_format("%s/target/lmdb-ulmdb", ULMDB_SOURCE_ROOT);
    if (!options.modern) options.modern = build_format("%s/support/lmdb/vendor", ULMDB_SOURCE_ROOT);
    options.legacy = build_format("%s/.cache/ulmdb-compat/lmdb-0.9.31", home);
    for (int index = 1; index < count; ++index) {
        const char *flag = arguments[index];
        if (!strcmp(flag, "--profile")) options.profile = argument(&index, count, arguments);
        else if (!strcmp(flag, "--output")) options.output = argument(&index, count, arguments);
        else if (!strcmp(flag, "--lmdb-source")) options.modern = argument(&index, count, arguments);
        else if (!strcmp(flag, "--legacy-source")) options.legacy = argument(&index, count, arguments);
        else if (!strcmp(flag, "--no-cargo")) options.no_cargo = 1;
        else if (!strcmp(flag, "--fetch-legacy")) options.fetch_legacy = 1;
        else if (!strcmp(flag, "--help") || !strcmp(flag, "-h")) { usage(stdout); exit(0); }
        else { usage(stderr); build_fail("unknown option: %s", flag); }
    }
    if (strcmp(options.profile, "debug") && strcmp(options.profile, "release"))
        build_fail("--profile must be debug or release");
    options.output = build_absolute(options.output);
    options.modern = build_absolute(options.modern);
    options.legacy = build_absolute(options.legacy);
    return options;
}

static void publish(const char *temporary, const char *target) {
    if (rename(temporary, target))
        build_fail("cannot install %s: %s", target, strerror(errno));
}

static void build_library(const char *compiler, const char *here, const char *archive,
                          const char *library) {
#if defined(__APPLE__)
    const char *soname = "liblmdb.0.dylib";
    char *target = build_format("%s/%s", library, soname);
    char *temporary = build_format("%s.build-%ld", target, (long)getpid());
    char *exports = build_format("%s/exports.darwin", here);
    /* Actual Rust1.94 Apple native-static-libs: -lSystem -lc -lm. Use separate
     * linker arguments so spaces/commas in archive/export paths stay intact. */
    char *arguments[] = {"-dynamiclib", "-Wl,-install_name,@rpath/liblmdb.0.dylib",
        "-Wl,-compatibility_version,1.0.0", "-Wl,-current_version,1.0.0",
        "-Xlinker", "-exported_symbols_list", "-Xlinker", exports,
        "-Xlinker", "-force_load", "-Xlinker", (char *)archive,
        "-lSystem", "-lc", "-lm", "-o", temporary, NULL};
    const char *aliases[] = {"liblmdb.dylib", "liblmdb.1.dylib", "libulmdb_c.dylib",
        "liblmdb.so", "liblmdb.so.0", "liblmdb.so.1", "libulmdb_c.so"};
#else
    const char *soname = "liblmdb.so.0";
    char *target = build_format("%s/%s", library, soname);
    char *temporary = build_format("%s.build-%ld", target, (long)getpid());
    char *exports = build_format("-Wl,--version-script=%s/exports.map", here);
    char *arguments[] = {"-shared", "-Wl,-soname,liblmdb.so.0", exports, "-Wl,--whole-archive",
                         (char *)archive, "-Wl,--no-whole-archive", "-ldl", "-lpthread", "-lm",
                         "-lrt", "-lutil", "-o", temporary, NULL};
    const char *aliases[] = {"liblmdb.so", "liblmdb.so.1", "libulmdb_c.so"};
#endif
    build_compile(compiler, arguments);
    publish(temporary, target);
    for (size_t index = 0; index < sizeof aliases / sizeof aliases[0]; ++index) {
        char *alias = build_format("%s/%s", library, aliases[index]);
        if (unlink(alias) && errno != ENOENT) build_fail("cannot replace library alias: %s", alias);
        if (symlink(soname, alias)) build_fail("cannot create library alias: %s", alias);
        free(alias);
    }
    char *static_library = build_format("%s/liblmdb.a", library);
    build_copy(archive, static_library, 0644);
    free(static_library);
    free(exports);
    free(temporary);
    free(target);
}

static void build_exporter(const char *compiler, const char *here, const char *binary,
                           const struct build_source *source, const char *name) {
    char *target = build_format("%s/%s", binary, name);
    char *temporary = build_format("%s.build-%ld", target, (long)getpid());
    char *exporter = build_format("%s/lmdb_export.c", here);
    char *mdb = build_format("%s/mdb.c", source->directory);
    char *midl = build_format("%s/midl.c", source->directory);
    /* Upstream sources retain their own warning policy. */
    char *arguments[] = {"-std=c11", "-O2", "-pthread", TOOL_PLATFORM_DEFINE, "-I",
                         (char *)source->directory, exporter, mdb, midl, "-o", temporary, NULL};
    build_compile(compiler, arguments);
    publish(temporary, target);
    free(midl); free(mdb); free(exporter); free(temporary); free(target);
}

/* Install the byte-for-byte pinned upstream headers as explicit build profiles.
 * Runtime catalog/lifetime selection is explicit too: launcher --abi or the
 * existing environment setting. A persisted environment marker keeps priority. */
static void build_header_profile(const char *output, const struct build_source *source,
                                 const char *major, const char *abi) {
    char *directory = build_format("%s/include/lmdb-%s", output, major);
    build_mkdirs(directory);
    char *from = build_format("%s/lmdb.h", source->directory);
    char *to = build_format("%s/lmdb.h", directory);
    build_copy(from, to, 0644);
    char *metadata = build_format("%s/lib/pkgconfig", output);
    build_mkdirs(metadata);
    char *target = build_format("%s/lmdb-ulmdb-%s.pc", metadata, major);
    char *temporary = build_format("%s.build-%ld", target, (long)getpid());
    FILE *file = fopen(temporary, "wx");
    if (!file) build_fail("cannot create %s: %s", temporary, strerror(errno));
    if (fprintf(file,
        "prefix=${pcfiledir}/../..\nlibdir=${prefix}/lib\nincludedir=${prefix}/include\n"
        "ulmdb_abi=%s\nName: ULMDB LMDB %s API\n"
        "Description: ULMDB with the pinned LMDB %s header profile\nVersion: 0.1.0\n"
        "Libs: -L${libdir} -llmdb\nLibs.private: %s\n"
        "Cflags: -I${includedir}/lmdb-%s\n", abi, major, abi, TOOL_PRIVATE_LIBS, major) < 0 || fclose(file))
        build_fail("cannot write %s: %s", temporary, strerror(errno));
    publish(temporary, target);
    free(temporary); free(target); free(metadata); free(to); free(from); free(directory);
}

static void build_tool(const char *compiler, const char *here, const char *binary,
                       const char *name, const char *const sources[], int dynamic_loader) {
    char *target = build_format("%s/%s", binary, name);
    char *temporary = build_format("%s.build-%ld", target, (long)getpid());
    char *arguments[64] = {"-std=c11", "-O2", TOOL_PLATFORM_DEFINE, "-Wall", "-Wextra", "-Werror"};
    size_t count = 6;
    for (size_t index = 0; sources[index]; ++index) {
        if (count >= 55) build_fail("too many native tool source modules");
        arguments[count++] = build_format("%s/%s", here, sources[index]);
    }
    size_t end_sources = count;
#if defined(__APPLE__)
    (void)dynamic_loader; /* dlopen is part of libSystem. */
#else
    if (dynamic_loader) arguments[count++] = "-ldl";
#endif
    arguments[count++] = "-o";
    arguments[count++] = temporary;
    arguments[count] = NULL;
    build_compile(compiler, arguments);
    publish(temporary, target);
    for (size_t index = 6; index < end_sources; ++index) free(arguments[index]);
    free(temporary);
    free(target);
}

/* The installed command uses the package SONAME and remains relocatable. */
static void build_operator(const char *compiler,const char *here,const char *binary,const char *library){
    char *target=build_format("%s/ulmdb-tool",binary);
    char *temporary=build_format("%s.build-%ld",target,(long)getpid());
    char *main_source=build_format("%s/operator.c",here);
    char *stream_source=build_format("%s/operator_stream.c",here);
    char *fs_source=build_format("%s/operator_fs.c",here);
    char *inspect_source=build_format("%s/operator_inspect.c",here);
    char *include=build_format("-I%s/clients/c/include",ULMDB_SOURCE_ROOT);
    char *link=build_format("-L%s",library);
    char *arguments[]={"-std=c11","-O2",TOOL_PLATFORM_DEFINE,"-Wall","-Wextra","-Werror",include,
        main_source,stream_source,fs_source,inspect_source,link,"-llmdb",TOOL_OPERATOR_RPATH,"-o",temporary,NULL};
    build_compile(compiler,arguments);publish(temporary,target);
    char *guide=build_format("%s/OPERATOR.md",here);
    char *installed_guide=build_format("%s/../OPERATOR.md",binary);
    build_copy(guide,installed_guide,0644);free(installed_guide);free(guide);
    free(link);free(include);free(inspect_source);free(fs_source);free(stream_source);free(main_source);free(temporary);free(target);
}

int main(int count, char **arguments) {
#if !defined(__linux__) && !defined(__APPLE__)
    build_fail("the native liblmdb package targets Linux or macOS");
#endif
    struct options options = parse(count, arguments);
    const char *compiler = getenv("CC");
    if (!compiler) compiler = "cc";
    char *here = build_format("%s/clients/lmdb-ulmdb", ULMDB_SOURCE_ROOT);
    struct build_source modern, legacy;
    build_modern_source(ULMDB_SOURCE_ROOT, options.modern, &modern);
    build_legacy_source(here, options.legacy, options.fetch_legacy, &legacy);
    if (chdir(ULMDB_SOURCE_ROOT)) build_fail("cannot enter repository: %s", strerror(errno));
    if (!options.no_cargo) {
        char *cargo[] = {"cargo", "build", "--locked", "-p", "ulmdb-c", NULL, NULL};
        if (!strcmp(options.profile, "release")) cargo[5] = "--release";
        build_run(cargo);
    }
    const char *configured_target = getenv("CARGO_TARGET_DIR");
    char *target = build_absolute(configured_target ? configured_target : "target");
    char *archive = build_format("%s/%s/libulmdb_c.a", target, options.profile);
    if (access(archive, R_OK)) build_fail("ULMDB static library missing: %s", archive);
    char *library = build_format("%s/lib", options.output);
    char *binary = build_format("%s/bin", options.output);
    char *include = build_format("%s/include", options.output);
    build_mkdirs(library); build_mkdirs(binary); build_mkdirs(include);
    build_library(compiler, here, archive, library);
    const char *headers[] = {"lmdb.h", "ulmdb.h"};
    for (size_t index = 0; index < 2; ++index) {
        char *from = build_format("%s/clients/c/include/%s", ULMDB_SOURCE_ROOT, headers[index]);
        char *to = build_format("%s/%s", include, headers[index]);
        build_copy(from, to, 0644);
        free(from); free(to);
    }
    build_header_profile(options.output, &legacy, "0.9", "0.9.31");
    build_header_profile(options.output, &modern, "1.0", "1.0.2");
    build_exporter(compiler, here, binary, &modern, "lmdb-export");
    build_exporter(compiler, here, binary, &legacy, "lmdb-export-0.9");
    build_notices(here, options.output, &modern, &legacy);
    const char *migration[] = {"migrate.c", "migrate_stream.c", "migrate_fs.c", NULL};
    const char *launcher[] = {"convert-lmdb-to-ulmdb.c", NULL};
    build_tool(compiler, here, binary, "ulmdb-migrate", migration, 1);
    build_tool(compiler, here, binary, "convert-lmdb-to-ulmdb", launcher, 0);
    build_operator(compiler,here,binary,library);
    char *old_launcher = build_format("%s/lmdb-ulmdb", binary);
    if (unlink(old_launcher) && errno != ENOENT) build_fail("cannot remove old launcher: %s", strerror(errno));
    printf("Adapter package: %s\n", options.output);
    printf("Existing dynamic applications: %s/convert-lmdb-to-ulmdb your-application\n", binary);
    printf("Operator commands: %s/ulmdb-tool --help\n",binary);
    free(old_launcher); free(include); free(binary); free(library);
    free(archive); free(target); free(here);
    return 0;
}
