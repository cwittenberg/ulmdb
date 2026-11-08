/* Adapt only argv: keep the public incremental fixture's body unchanged. */
#ifdef _GNU_SOURCE
#undef _GNU_SOURCE
#endif
#define main incremental_fixture_main
#include "../../c/tests/incremental.c"
#undef main

int main(int argc, char **argv) {
    CHECK(argc == 2);
    char source[4096], restored[4096], path_delta[4096], fd_delta[4096];
    const char *names[] = {"source", "restored", "path-delta", "fd-delta"};
    char *paths[] = {source, restored, path_delta, fd_delta};
    for (size_t i = 0; i < sizeof paths / sizeof *paths; ++i) {
        int n = snprintf(paths[i], sizeof source, "%s/%s", argv[1], names[i]);
        CHECK(n > 0 && (size_t)n < sizeof source);
    }
    char *arguments[] = {argv[0], source, restored, path_delta, fd_delta, NULL};
    return incremental_fixture_main(5, arguments);
}
