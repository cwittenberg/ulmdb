/* Differential inputs only: matched precreated directories and NOSUBDIR. */
#include <errno.h>
#include <sys/stat.h>
#define main parity_first_ids_leaf_main
#include "parity_first_ids.c"
#undef main

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    if (mkdir(argv[1], 0700) && errno != EEXIST) return 2;
    puts("first-ids-suite-version=1");
    unsigned suite_count = 0;
    for (unsigned nosubdir = 0; nosubdir < 2; ++nosubdir) {
        for (unsigned notls = 0; notls < 2; ++notls) {
            char directory[4096], path[4096], flags_arg[32];
            int n = snprintf(directory, sizeof directory, "%s/%s-notls%u",
                             argv[1], nosubdir ? "nosubdir" : "precreated", notls);
            if (n < 0 || (size_t)n >= sizeof directory || mkdir(directory, 0700)) return 2;
            n = snprintf(path, sizeof path, "%s/database", directory);
            if (n < 0 || (size_t)n >= sizeof path) return 2;
            if (!nosubdir && mkdir(path, 0700)) return 2;
            unsigned flags = (nosubdir ? MDB_NOSUBDIR : 0) | (notls ? MDB_NOTLS : 0);
            n = snprintf(flags_arg, sizeof flags_arg, "%u", flags);
            if (n < 0 || (size_t)n >= sizeof flags_arg) return 2;
            char *leaf_argv[] = {argv[0], path, flags_arg, NULL};
            printf("scenario=first-ids-%s-notls%u\n",
                   nosubdir ? "nosubdir" : "precreated", notls);
            int result = parity_first_ids_leaf_main(3, leaf_argv);
            if (result) return result;
            ++suite_count;
        }
    }
    if (suite_count != 4) return 1;
    puts("first-ids-suite-complete journeys=4");
    return 0;
}
