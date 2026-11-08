/* SHA-256 source integrity verification, confined to development packaging. */
#include "build_support.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

struct hash_state {
    uint32_t words[8];
    unsigned char block[64];
    size_t used;
    uint64_t bytes;
};

static uint32_t rotate(uint32_t value, unsigned bits) {
    return (value >> bits) | (value << (32 - bits));
}

static void hash_block(struct hash_state *state) {
    static const uint32_t constants[64] = {
        0x428a2f98,0x71374491,0xb5c0fbcf,0xe9b5dba5,0x3956c25b,0x59f111f1,0x923f82a4,0xab1c5ed5,
        0xd807aa98,0x12835b01,0x243185be,0x550c7dc3,0x72be5d74,0x80deb1fe,0x9bdc06a7,0xc19bf174,
        0xe49b69c1,0xefbe4786,0x0fc19dc6,0x240ca1cc,0x2de92c6f,0x4a7484aa,0x5cb0a9dc,0x76f988da,
        0x983e5152,0xa831c66d,0xb00327c8,0xbf597fc7,0xc6e00bf3,0xd5a79147,0x06ca6351,0x14292967,
        0x27b70a85,0x2e1b2138,0x4d2c6dfc,0x53380d13,0x650a7354,0x766a0abb,0x81c2c92e,0x92722c85,
        0xa2bfe8a1,0xa81a664b,0xc24b8b70,0xc76c51a3,0xd192e819,0xd6990624,0xf40e3585,0x106aa070,
        0x19a4c116,0x1e376c08,0x2748774c,0x34b0bcb5,0x391c0cb3,0x4ed8aa4a,0x5b9cca4f,0x682e6ff3,
        0x748f82ee,0x78a5636f,0x84c87814,0x8cc70208,0x90befffa,0xa4506ceb,0xbef9a3f7,0xc67178f2
    };
    uint32_t schedule[64];
    for (unsigned index = 0; index < 16; ++index) {
        unsigned char *word = state->block + 4 * index;
        schedule[index] = (uint32_t)word[0] << 24 | (uint32_t)word[1] << 16 |
                          (uint32_t)word[2] << 8 | word[3];
    }
    for (unsigned index = 16; index < 64; ++index) {
        uint32_t x = schedule[index-15], y = schedule[index-2];
        schedule[index] = schedule[index-16] + (rotate(x,7) ^ rotate(x,18) ^ (x >> 3)) +
                          schedule[index-7] + (rotate(y,17) ^ rotate(y,19) ^ (y >> 10));
    }
    uint32_t a=state->words[0], b=state->words[1], c=state->words[2], d=state->words[3];
    uint32_t e=state->words[4], f=state->words[5], g=state->words[6], h=state->words[7];
    for (unsigned index = 0; index < 64; ++index) {
        uint32_t first = h + (rotate(e,6) ^ rotate(e,11) ^ rotate(e,25)) +
                         ((e & f) ^ (~e & g)) + constants[index] + schedule[index];
        uint32_t second = (rotate(a,2) ^ rotate(a,13) ^ rotate(a,22)) +
                          ((a & b) ^ (a & c) ^ (b & c));
        h=g; g=f; f=e; e=d+first; d=c; c=b; b=a; a=first+second;
    }
    state->words[0]+=a; state->words[1]+=b; state->words[2]+=c; state->words[3]+=d;
    state->words[4]+=e; state->words[5]+=f; state->words[6]+=g; state->words[7]+=h;
}

static void hash_update(struct hash_state *state, const unsigned char *data, size_t length) {
    state->bytes += length;
    while (length) {
        size_t amount = 64 - state->used;
        if (amount > length) amount = length;
        memcpy(state->block + state->used, data, amount);
        state->used += amount;
        data += amount;
        length -= amount;
        if (state->used == 64) {
            hash_block(state);
            state->used = 0;
        }
    }
}

void build_hash(const char *path, uint64_t *bytes, char digest[65]) {
    struct hash_state state = {{0x6a09e667,0xbb67ae85,0x3c6ef372,0xa54ff53a,
                                0x510e527f,0x9b05688c,0x1f83d9ab,0x5be0cd19},{0},0,0};
    int descriptor = open(path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    struct stat metadata;
    if (descriptor < 0 || fstat(descriptor, &metadata) || !S_ISREG(metadata.st_mode))
        build_fail("expected regular pinned source: %s", path);
    unsigned char buffer[65536];
    for (;;) {
        ssize_t count = read(descriptor, buffer, sizeof buffer);
        if (count < 0 && errno == EINTR) continue;
        if (count < 0) build_fail("cannot hash %s: %s", path, strerror(errno));
        if (!count) break;
        hash_update(&state, buffer, (size_t)count);
    }
    if (close(descriptor)) build_fail("cannot close %s", path);
    *bytes = state.bytes;
    uint64_t bits = state.bytes * 8;
    unsigned char marker = 0x80, zero = 0;
    hash_update(&state, &marker, 1);
    while (state.used != 56) hash_update(&state, &zero, 1);
    unsigned char tail[8];
    for (unsigned index = 0; index < 8; ++index) tail[index] = (unsigned char)(bits >> (56 - index * 8));
    hash_update(&state, tail, 8);
    for (unsigned index = 0; index < 8; ++index)
        snprintf(digest + index * 8, 9, "%08x", state.words[index]);
}
