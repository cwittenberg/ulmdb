#define _GNU_SOURCE
/* Profiling-only Linux/x86_64 sampler. Never linked into the database. */
#include <errno.h>
#include <fcntl.h>
#include <link.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/time.h>
#include <time.h>
#include <ucontext.h>
#include <unistd.h>

#if !defined(__linux__) || !defined(__x86_64__)
#error "This profiling tool requires Linux x86_64 ucontext REG_RIP."
#endif
_Static_assert(ATOMIC_INT_LOCK_FREE == 2, "handler atomics must be lock-free");
_Static_assert(ATOMIC_LLONG_LOCK_FREE == 2, "handler atomics must be lock-free");

struct sample { uint64_t monotonic_ns, pc; };
static struct sample *samples;
static unsigned capacity, interval_us;
static _Atomic unsigned enabled, active;
static _Atomic unsigned long long next_sample;
static uint64_t started_ns;
static char output[4096], executable[4096];

static uint64_t now_ns(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return 0;
    return (uint64_t)ts.tv_sec * UINT64_C(1000000000) + (uint64_t)ts.tv_nsec;
}

static void sample_pc(int signal_number, siginfo_t *info, void *context) {
    (void)signal_number;
    (void)info;
    int saved_errno = errno;
    /* The cross-atomic order also closes the finalizer/handler entry race. */
    atomic_fetch_add_explicit(&active, 1, memory_order_seq_cst);
    if (atomic_load_explicit(&enabled, memory_order_seq_cst)) {
        unsigned long long index = atomic_fetch_add_explicit(
            &next_sample, 1, memory_order_relaxed);
        if (index < capacity) {
            const ucontext_t *uc = context;
            samples[index].monotonic_ns = now_ns();
            samples[index].pc = (uint64_t)uc->uc_mcontext.gregs[REG_RIP];
        }
    }
    atomic_fetch_sub_explicit(&active, 1, memory_order_seq_cst);
    errno = saved_errno;
}

static unsigned option(const char *name, unsigned fallback,
                       unsigned minimum, unsigned maximum) {
    const char *value = getenv(name);
    if (!value || !*value) return fallback;
    char *end;
    errno = 0;
    unsigned long number = strtoul(value, &end, 10);
    if (errno || *end || number < minimum || number > maximum) return 0;
    return (unsigned)number;
}

__attribute__((constructor)) static void start_sampler(void) {
    const char *path = getenv("ULMDB_PC_PROFILE");
    if (!path || !*path) return;
    if (strlen(path) >= sizeof(output) - 16) {
        fprintf(stderr, "pc sampler: output path is too long\n");
        return;
    }
    interval_us = option("ULMDB_PC_INTERVAL_US", 1000, 100, 1000000);
    capacity = option("ULMDB_PC_MAX_SAMPLES", 1000000, 1, 10000000);
    if (!interval_us || !capacity) {
        fprintf(stderr, "pc sampler: invalid interval or sample capacity\n");
        return;
    }
    struct sigaction old_action;
    struct itimerval old_timer;
    if (sigaction(SIGPROF, NULL, &old_action) != 0 ||
        getitimer(ITIMER_PROF, &old_timer) != 0 ||
        old_action.sa_handler != SIG_DFL ||
        old_timer.it_value.tv_sec || old_timer.it_value.tv_usec ||
        old_timer.it_interval.tv_sec || old_timer.it_interval.tv_usec) {
        fprintf(stderr, "pc sampler: SIGPROF or ITIMER_PROF already in use\n");
        return;
    }
    samples = calloc(capacity, sizeof(*samples));
    if (!samples) {
        fprintf(stderr, "pc sampler: cannot allocate bounded sample buffer\n");
        return;
    }
    ssize_t length = readlink("/proc/self/exe", executable, sizeof(executable) - 1);
    if (length >= 0) executable[length] = 0;
    strcpy(output, path);
    struct sigaction action = {0};
    sigemptyset(&action.sa_mask);
    action.sa_sigaction = sample_pc;
    action.sa_flags = SA_SIGINFO | SA_RESTART;
    if (sigaction(SIGPROF, &action, NULL) != 0) {
        fprintf(stderr, "pc sampler: cannot install signal handler\n");
        output[0] = 0;
        return;
    }
    struct itimerval timer = {0};
    timer.it_interval.tv_sec = interval_us / 1000000;
    timer.it_interval.tv_usec = interval_us % 1000000;
    timer.it_value = timer.it_interval;
    started_ns = now_ns();
    atomic_store_explicit(&enabled, 1, memory_order_seq_cst);
    if (setitimer(ITIMER_PROF, &timer, NULL) != 0) {
        atomic_store_explicit(&enabled, 0, memory_order_seq_cst);
        output[0] = 0;
        fprintf(stderr, "pc sampler: cannot arm CPU timer\n");
    }
}

static void json_string(FILE *file, const char *value) {
    fputc('"', file);
    for (const unsigned char *p = (const unsigned char *)value; *p; ++p) {
        if (*p == '"' || *p == '\\') { fputc('\\', file); fputc(*p, file); }
        else if (*p < 32) fprintf(file, "\\u%04x", *p);
        else fputc(*p, file);
    }
    fputc('"', file);
}

struct modules_context { FILE *file; bool first; };
static int write_module(struct dl_phdr_info *info, size_t size, void *data) {
    (void)size;
    struct modules_context *context = data;
    FILE *file = context->file;
    if (!context->first) fputs(",\n", file);
    context->first = false;
    fputs("    {\"path\": ", file);
    json_string(file, *info->dlpi_name ? info->dlpi_name : executable);
    fprintf(file, ", \"main\": %s, \"load_bias\": \"0x%llx\", \"exec_ranges\": [",
            *info->dlpi_name ? "false" : "true",
            (unsigned long long)info->dlpi_addr);
    bool first = true;
    for (ElfW(Half) i = 0; i < info->dlpi_phnum; ++i) {
        const ElfW(Phdr) *p = &info->dlpi_phdr[i];
        if (p->p_type != PT_LOAD || !(p->p_flags & PF_X)) continue;
        if (!first) fputs(", ", file);
        first = false;
        fprintf(file, "[\"0x%llx\", \"0x%llx\"]",
                (unsigned long long)(info->dlpi_addr + p->p_vaddr),
                (unsigned long long)(info->dlpi_addr + p->p_vaddr + p->p_memsz));
    }
    fputs("]}", file);
    return 0;
}

static FILE *new_file(const char *path) {
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (fd < 0) return NULL;
    FILE *file = fdopen(fd, "w");
    if (!file) close(fd);
    return file;
}

__attribute__((destructor)) static void stop_sampler(void) {
    if (!output[0]) return;
    atomic_store_explicit(&enabled, 0, memory_order_seq_cst);
    const struct itimerval stopped = {0};
    setitimer(ITIMER_PROF, &stopped, NULL);
    /* A late SIGPROF sees enabled=0. Keep the handler/buffer until process exit. */
    while (atomic_load_explicit(&active, memory_order_seq_cst)) {
        const struct timespec pause = {0, 1000000};
        nanosleep(&pause, NULL);
    }
    uint64_t finished_ns = now_ns();
    unsigned long long seen = atomic_load_explicit(&next_sample, memory_order_acquire);
    unsigned long long count = seen < capacity ? seen : capacity;
    FILE *file = new_file(output);
    if (!file) {
        fprintf(stderr, "pc sampler: cannot create %s: %s\n", output, strerror(errno));
        return;
    }
    fputs("monotonic_ns,elapsed_ns,pc\n", file);
    for (unsigned long long i = 0; i < count; ++i) {
        fprintf(file, "%llu,%llu,0x%llx\n",
                (unsigned long long)samples[i].monotonic_ns,
                (unsigned long long)(samples[i].monotonic_ns - started_ns),
                (unsigned long long)samples[i].pc);
    }
    if (fclose(file) != 0) fprintf(stderr, "pc sampler: sample write failed\n");
    char metadata[sizeof(output) + 16];
    snprintf(metadata, sizeof(metadata), "%s.meta.json", output);
    file = new_file(metadata);
    if (!file) {
        fprintf(stderr, "pc sampler: cannot create metadata: %s\n", strerror(errno));
        return;
    }
    fprintf(file, "{\n  \"version\": 1, \"timer\": \"ITIMER_PROF\",\n"
            "  \"interval_us\": %u, \"capacity\": %u, \"sample_buffer_bytes\": %zu,\n"
            "  \"signals_seen\": %llu, \"samples\": %llu, \"dropped_at_capacity\": %llu,\n"
            "  \"started_monotonic_ns\": %llu, \"finished_monotonic_ns\": %llu,\n"
            "  \"executable\": ", interval_us, capacity, capacity * sizeof(*samples),
            seen, count, seen - count, (unsigned long long)started_ns,
            (unsigned long long)finished_ns);
    json_string(file, executable);
    fputs(",\n  \"modules\": [\n", file);
    struct modules_context context = {file, true};
    dl_iterate_phdr(write_module, &context);
    fputs("\n  ]\n}\n", file);
    if (fclose(file) != 0) fprintf(stderr, "pc sampler: metadata write failed\n");
}
