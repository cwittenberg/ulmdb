/* Compose the frozen CLI leaf for the two-argument parity runner contract. */
#define main parity_prepare_leaf_main
#include "parity_prepare.c"
#undef main

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 2;
    static const char *const suite_modes[] = {
        "clean-fresh", "clean", "readonly", "dirty-kv", "kv-identical",
        "main-identical", "main-flags-identical", "main-flags-new",
        "dup-equivalent", "dup-nodupdata", "dup-appenddup", "kv-nooverwrite",
        "dup-nooverwrite", "kv-append-rejected", "empty-main-clear",
        "empty-named-clear", "reserve", "created-dbi", "nested-clean",
        "nested-dirty", "nested-abort",
    };
    puts("prepare-suite-version=1");
    unsigned suite_count = 0;
    for (size_t mode = 0; mode < sizeof suite_modes / sizeof suite_modes[0]; ++mode) {
        for (unsigned notls = 0; notls < 2; ++notls) {
            for (unsigned abort_final = 0; abort_final < 2; ++abort_final) {
                char path[4096];
                int n = snprintf(path, sizeof path, "%s/%s-notls%u-abort%u",
                                 argv[1], suite_modes[mode], notls, abort_final);
                if (n < 0 || (size_t)n >= sizeof path) return 2;
                char notls_arg[2] = {(char)('0' + notls), '\0'};
                char abort_arg[2] = {(char)('0' + abort_final), '\0'};
                char *leaf_argv[] = {argv[0], path, (char *)suite_modes[mode],
                                     notls_arg, abort_arg, NULL};
                printf("scenario=prepare-%s-notls%u-abort%u\n",
                       suite_modes[mode], notls, abort_final);
                int result = parity_prepare_leaf_main(5, leaf_argv);
                if (result) return result;
                ++suite_count;
            }
        }
    }
    if (suite_count != 84) return 1;
    puts("prepare-suite-complete journeys=84");
    return 0;
}
