/* Two-argument suite adapter for the frozen 55-case rollback leaf. */
#define main parity_rollback_leaf_main
#include "parity_rollback.c"
#undef main

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    puts("rollback-suite-version=1");
    int result = parity_rollback_leaf_main(2, argv);
    if (result) return result;
    puts("rollback-suite-complete journeys=55");
    return 0;
}
