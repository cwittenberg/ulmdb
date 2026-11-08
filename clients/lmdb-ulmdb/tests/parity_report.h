/* Public evidence for a precisely bounded functional compatibility profile. */
#ifndef ULMDB_PARITY_REPORT_H
#define ULMDB_PARITY_REPORT_H
#include "../build_support.h"
#include <stdint.h>
#include <stddef.h>

#define PARITY_MAX_RESULTS 96
#define PARITY_MAX_SYMBOLS 128
#define PARITY_MAX_REFERENCES 3
struct parity_result {
    const char *name, *abi;
    uint64_t seed, operations, trace_bytes;
    int passed, reference_status, adapter_status;
    char executable_sha256[65], adapter_executable_sha256[65];
    char reference_sha256[65], adapter_sha256[65];
};
struct parity_symbols {
    char names[PARITY_MAX_SYMBOLS][96];
    unsigned count, supported;
    unsigned char present[PARITY_MAX_SYMBOLS];
};
struct parity_report {
    const char *directory, *compiler, *profile;
    struct build_source sources[PARITY_MAX_REFERENCES];
    const char *versions[PARITY_MAX_REFERENCES];
    unsigned reference_count;
    char adapter_sha256[65];
    uint64_t adapter_bytes;
    struct parity_result results[PARITY_MAX_RESULTS];
    size_t count;
    struct parity_symbols common, modern_extensions;
    int complete;
};
int parity_report_write(const struct parity_report *);
int parity_report_passed(const struct parity_report *);
#endif
