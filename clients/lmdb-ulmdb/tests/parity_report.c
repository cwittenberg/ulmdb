/* JSON receipt and its human-readable projection share the same result set. */
#include "parity_report.h"
#include <errno.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

static void json_string(FILE *file, const char *text) {
    fputc('"', file);
    for (const unsigned char *p = (const unsigned char *)(text ? text : ""); *p; ++p) {
        if (*p == '"' || *p == '\\') { fputc('\\', file); fputc(*p, file); }
        else if (*p < 0x20 || *p >= 0x80) fprintf(file, "\\u%04x", *p);
        else fputc(*p, file);
    }
    fputc('"', file);
}
int parity_report_passed(const struct parity_report *report) {
    if (!report->complete || !report->count || !report->common.count ||
        report->common.count != report->common.supported) return 0;
    for (size_t i = 0; i < report->count; ++i)
        if (!report->results[i].passed) return 0;
    return 1;
}
static void source_json(FILE *file, const struct build_source *source, const char *version) {
    fputs("{\"version\":", file); json_string(file, version);
    fputs(",\"commit\":", file); json_string(file, source->commit);
    fputs(",\"files\":[", file);
    for (size_t i = 0; i < source->count; ++i) {
        const struct build_pin *pin = source->files + i;
        if (i) fputc(',', file);
        fputs("{\"name\":", file); json_string(file, pin->name);
        fprintf(file, ",\"bytes\":%" PRIu64 ",\"sha256\":", pin->bytes);
        json_string(file, pin->sha256); fputc('}', file);
    }
    fputs("]}", file);
}
static void symbols_json(FILE *file, const struct parity_symbols *symbols) {
    fprintf(file, "{\"declared\":%u,\"exported\":%u,\"symbols\":[", symbols->count, symbols->supported);
    for (unsigned i = 0; i < symbols->count; ++i) {
        if (i) fputc(',', file);
        fputs("{\"name\":", file); json_string(file, symbols->names[i]);
        fprintf(file, ",\"exported\":%s}", symbols->present[i] ? "true" : "false");
    }
    fputs("]}", file);
}
static void receipt(FILE *file, const struct parity_report *report, const char *generated) {
    uint64_t operations = 0;
    for (size_t i = 0; i < report->count; ++i) operations += report->results[i].operations;
    fputs("{\n  \"schema\":1,\n  \"profile\":", file);
    json_string(file, report->profile);
    fputs(",\n  \"generated_utc\":", file);
    json_string(file, generated);
    fprintf(file, ",\n  \"complete\":%s,\n  \"passed\":%s,\n  \"compiler\":",
            report->complete ? "true" : "false", parity_report_passed(report) ? "true" : "false");
    json_string(file, report->compiler);
    fprintf(file, ",\n  \"randomized_operations_total\":%" PRIu64, operations);
    fprintf(file, ",\n  \"randomized_operations_per_backend\":%" PRIu64,
            report->reference_count ? operations / report->reference_count : 0);
    fputs(",\n  \"method\":\"Behavioral scenarios use unchanged executables and compare logical traces/return codes byte for byte. The header ABI probe compiles the same source against each header.\",\n", file);
    fputs("  \"scope\":{\"platform\":\"Linux\",\"api\":\"LMDB 0.9 public API plus selected LMDB 1.0 behavior\","
          "\"keys_and_duplicate_values_max_bytes\":511,\"ordinary_values_max_bytes\":67108864,"
          "\"storage\":\"Native ULMDB with preserved-source automatic LMDB import\","
          "\"excluded\":[\"Raw LMDB page/file-format identity\",\"Fixed-address maps and persisted raw pointers\","
          "\"Physical page-statistics identity and identical map-full thresholds\",", file);
    json_string(file, !strcmp(report->profile, "lmdb-features-v2") ?
        "LMDB 1.0-only function extensions except mdb_cursor_is_db and mdb_txn_flags" :
        "LMDB 1.0-only extensions except mdb_cursor_is_db");
    fputs(",\"Performance equivalence\",\"Universal formal correctness or durability qualification\"]},\n", file);
    fprintf(file, "  \"adapter\":{\"artifact\":\"lib/liblmdb.so.0\",\"bytes\":%" PRIu64 ",\"sha256\":", report->adapter_bytes);
    json_string(file, report->adapter_sha256); fputs("},\n  \"references\":[", file);
    for (unsigned i = 0; i < report->reference_count; ++i) {
        if (i) fputc(',', file);
        source_json(file, report->sources + i, report->versions[i]);
    }
    fputs("],\n  \"common_symbols\":", file); symbols_json(file, &report->common);
    fputs(",\n  \"modern_additional_symbols\":", file); symbols_json(file, &report->modern_extensions);
    fputs(",\n  \"results\":[\n", file);
    for (size_t i = 0; i < report->count; ++i) {
        const struct parity_result *result = report->results + i;
        fputs(i ? ",\n    {\"scenario\":" : "    {\"scenario\":", file);
        json_string(file, result->name); fputs(",\"reference\":", file); json_string(file, result->abi);
        fprintf(file, ",\"passed\":%s,\"reference_exit\":%d,\"adapter_exit\":%d,"
                "\"seed\":%" PRIu64 ",\"operations\":%" PRIu64 ",\"trace_bytes\":%" PRIu64 ",\"executable_sha256\":",
                result->passed ? "true" : "false", result->reference_status, result->adapter_status,
                result->seed, result->operations, result->trace_bytes);
        json_string(file, result->executable_sha256);
        fputs(",\"adapter_executable_sha256\":", file); json_string(file, result->adapter_executable_sha256);
        fputs(",\"reference_trace_sha256\":", file); json_string(file, result->reference_sha256);
        fputs(",\"adapter_trace_sha256\":", file); json_string(file, result->adapter_sha256);
        fputc('}', file);
    }
    fputs("\n  ]\n}\n", file);
}
static const struct parity_result *matching(const struct parity_report *report,
                                            const char *name, uint64_t seed, const char *abi) {
    for (size_t i = 0; i < report->count; ++i) {
        const struct parity_result *result = report->results + i;
        if (!strcmp(result->name, name) && result->seed == seed &&
            (!strcmp(result->abi, abi) ||
             (!strcmp(result->abi, "both") && strcmp(abi, "0.9.36")))) return result;
    }
    return NULL;
}
static const char *outcome(const struct parity_result *result) {
    return !result ? "NOT RUN" : result->passed ? "PASS" : "FAIL";
}
static void markdown(FILE *file, const struct parity_report *report, const char *generated) {
    fprintf(file, "# LMDB functional conformance\n\nGenerated %s. Profile: `%s`.\n\n", generated, report->profile);
    fprintf(file, "**%s**: %zu recorded checks. Each behavioral differential scenario runs an unchanged executable "
            "against the original LMDB shared library and the ULMDB replacement. Return codes and logical "
            "traces must match byte for byte. The [JSON receipt](receipt.json) records exact source commits, "
            "source hashes, executable hashes, trace hashes, compiler and adapter artifact.\n\n",
            parity_report_passed(report) ? "PASS" : "FAIL / INCOMPLETE", report->count);
    fputs("| Scenario |", file);
    for (unsigned i = 0; i < report->reference_count; ++i) fprintf(file, " LMDB %s |", report->versions[i]);
    fputs("\n| --- |", file);
    for (unsigned i = 0; i < report->reference_count; ++i) fputs(" --- |", file);
    fputs("\n| Common public C symbols |", file);
    for (unsigned i = 0; i < report->reference_count; ++i)
        fprintf(file, " %u/%u exported |", report->common.supported, report->common.count);
    fputc('\n', file);
    for (size_t i = 0; i < report->count; ++i) {
        const struct parity_result *result = report->results + i;
        int seen = 0;
        for (size_t j = 0; j < i; ++j)
            if (!strcmp(report->results[j].name, result->name) && report->results[j].seed == result->seed) seen = 1;
        if (seen) continue;
        fprintf(file, "| %s", result->name);
        if (result->operations) fprintf(file, " (seed %" PRIu64 ", %" PRIu64 " operations)", result->seed, result->operations);
        fputs(" |", file);
        for (unsigned reference = 0; reference < report->reference_count; ++reference)
            fprintf(file, " %s |", outcome(matching(report, result->name, result->seed, report->versions[reference])));
        fputc('\n', file);
    }
    fputs("\nThe compatibility profile covers the tested logical transaction, key/value, duplicate, "
          "cursor, ordering and migration behavior of the common API. It accepts automatic conversion "
          "to native ULMDB storage with the original LMDB data retained. Keys and duplicate values are "
          "limited to 511 bytes, and ordinary values to 64 MiB.\n\n"
          "Raw LMDB file/page identity, fixed-address mappings, persisted mapped pointers, identical "
          "physical statistics or capacity thresholds remain outside this profile. Only the named "
          "LMDB 1.0 extension scenarios in the table are qualified. The remaining parity gaps list missing APIs "
          "and behavioral differences. The test results establish empirical conformance for the recorded "
          "cases; they do not prove every possible execution, performance equivalence, or production "
          "durability. Modern-only symbols and their availability are enumerated in the receipt.\n\n"
          "Reproduce from the repository root:\n\n```sh\nmake -C clients/lmdb-ulmdb\n", file);
    fputs(!strcmp(report->profile, "lmdb-features-v2") ?
          "make -C clients/lmdb-ulmdb features\n```\n" :
          "make -C clients/lmdb-ulmdb parity\n```\n", file);
}
int parity_report_write(const struct parity_report *report) {
    char generated[32] = "unknown";
    time_t now = time(NULL);
    struct tm utc;
    if (gmtime_r(&now, &utc)) strftime(generated, sizeof generated, "%Y-%m-%dT%H:%M:%SZ", &utc);
    char *json_path = build_format("%s/receipt.json", report->directory);
    char *markdown_path = build_format("%s/REPORT.md", report->directory);
    FILE *json = fopen(json_path, "w"), *md = fopen(markdown_path, "w");
    if (!json || !md) {
        fprintf(stderr, "Cannot write parity evidence: %s\n", strerror(errno));
        if (json) fclose(json);
        if (md) fclose(md);
        free(json_path); free(markdown_path);
        return -1;
    }
    receipt(json, report, generated); markdown(md, report, generated);
    int failed = ferror(json) || ferror(md);
    if (fclose(json)) failed = 1;
    if (fclose(md)) failed = 1;
    free(json_path); free(markdown_path);
    return failed ? -1 : 0;
}
