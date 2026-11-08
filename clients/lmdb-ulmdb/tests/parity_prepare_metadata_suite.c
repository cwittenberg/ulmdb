/* The runner precreates DATABASE_ROOT; the frozen leaf creates its own root. */
#include <errno.h>
#define main parity_prepare_metadata_leaf_main
#include "parity_prepare_metadata.c"
#undef main

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 2;
    char path[4096];
    int n = snprintf(path, sizeof path, "%s/metadata", argv[1]);
    if (n < 0 || (size_t)n >= sizeof path) return 2;
    char *leaf_argv[] = {argv[0], path, NULL};
    puts("prepare-metadata-suite-version=1");
    int result = parity_prepare_metadata_leaf_main(2, leaf_argv);
    if (result) return result;
    puts("prepare-metadata-suite-complete journeys=32");
    return 0;
}
