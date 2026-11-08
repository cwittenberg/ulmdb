/* Optional OpenSSL3 module for ULMDB's MDB_crypto callback ABI. */
#include "lmdb.h"
#include <openssl/crypto.h>
#include <openssl/evp.h>
#include <openssl/opensslv.h>
#include <stdint.h>
#include <limits.h>
#include <string.h>
#if OPENSSL_VERSION_MAJOR < 3
#error OpenSSL3 or newer is required
#endif
#if defined(_WIN32)
#define MODULE_EXPORT __declspec(dllexport)
#else
#define MODULE_EXPORT __attribute__((visibility("default")))
#endif
static int hex_digit(unsigned char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}
/* This parses raw key material. It is deliberately not a passphrase KDF. */
static int raw_hex_key(const char *text, MDB_val *key) {
    unsigned char bytes[32] = {0};
    int result = 1;
    if (!text || !key || !key->mv_data || key->mv_size != sizeof(bytes)) goto done;
    if (strnlen(text, 65) != 64) goto done;
    for (size_t i = 0; i < sizeof(bytes); ++i) {
        int a = hex_digit((unsigned char)text[i * 2]);
        int b = hex_digit((unsigned char)text[i * 2 + 1]);
        if (a < 0 || b < 0) goto done;
        bytes[i] = (unsigned char)((a << 4) | b);
    }
    memcpy(key->mv_data, bytes, sizeof(bytes));
    result = 0;
done:
    OPENSSL_cleanse(bytes, sizeof(bytes));
    return result;
}
static int valid_span(const void *p, size_t n) {
    return n == 0 || (p && n <= UINTPTR_MAX - (uintptr_t)p);
}
static int overlaps(const void *a, size_t an, const void *b, size_t bn) {
    if (!an || !bn) return 0;
    return (uintptr_t)a < (uintptr_t)b + bn && (uintptr_t)b < (uintptr_t)a + an;
}
static int aes256gcm(const MDB_val *src, MDB_val *dst, const MDB_val *keys, int enc) {
    EVP_CIPHER *cipher = NULL;
    EVP_CIPHER_CTX *ctx = NULL;
    unsigned char final[EVP_MAX_BLOCK_LENGTH] = {0};
    int written = 0, tail = 0, result = 1;
    /* The callback ABI supplies exactly three live tuple entries. Shape checks
       precede output writes; no MDB_val pointer or length is ever replaced. */
    if (!src || !dst || !keys || (enc != 0 && enc != 1) ||
        src->mv_size != dst->mv_size || src->mv_size > INT_MAX ||
        keys[0].mv_size != 32 || keys[1].mv_size < 1 || keys[1].mv_size > 128 ||
        keys[2].mv_size != 16 || !valid_span(src->mv_data, src->mv_size) ||
        !valid_span(dst->mv_data, dst->mv_size) || !valid_span(keys[0].mv_data, 32) ||
        !valid_span(keys[1].mv_data, keys[1].mv_size) || !valid_span(keys[2].mv_data, 16)) return 1;
    if ((src->mv_data != dst->mv_data && overlaps(src->mv_data, src->mv_size, dst->mv_data, dst->mv_size)) ||
        overlaps(dst->mv_data, dst->mv_size, keys[0].mv_data, 32) ||
        overlaps(dst->mv_data, dst->mv_size, keys[1].mv_data, keys[1].mv_size) ||
        overlaps(dst->mv_data, dst->mv_size, keys[2].mv_data, 16) ||
        overlaps(keys[2].mv_data, 16, src->mv_data, src->mv_size) ||
        overlaps(keys[2].mv_data, 16, keys[0].mv_data, 32) ||
        overlaps(keys[2].mv_data, 16, keys[1].mv_data, keys[1].mv_size)) return 1;
    cipher = EVP_CIPHER_fetch(NULL, "AES-256-GCM", NULL);
    ctx = EVP_CIPHER_CTX_new();
    if (!cipher || !ctx) goto done;
    if (enc) {
        if (EVP_EncryptInit_ex2(ctx, cipher, NULL, NULL, NULL) != 1 ||
            EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_IVLEN, (int)keys[1].mv_size, NULL) != 1 ||
            EVP_EncryptInit_ex2(ctx, NULL, keys[0].mv_data, keys[1].mv_data, NULL) != 1 ||
            (src->mv_size && EVP_EncryptUpdate(ctx, dst->mv_data, &written, src->mv_data, (int)src->mv_size) != 1) ||
            written != (int)src->mv_size || EVP_EncryptFinal_ex(ctx, final, &tail) != 1 || tail != 0 ||
            EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_GET_TAG, 16, keys[2].mv_data) != 1) goto done;
    } else {
        if (EVP_DecryptInit_ex2(ctx, cipher, NULL, NULL, NULL) != 1 ||
            EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_IVLEN, (int)keys[1].mv_size, NULL) != 1 ||
            EVP_DecryptInit_ex2(ctx, NULL, keys[0].mv_data, keys[1].mv_data, NULL) != 1 ||
            EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_TAG, 16, keys[2].mv_data) != 1 ||
            (src->mv_size && EVP_DecryptUpdate(ctx, dst->mv_data, &written, src->mv_data, (int)src->mv_size) != 1) ||
            written != (int)src->mv_size || EVP_DecryptFinal_ex(ctx, final, &tail) != 1 || tail != 0) goto done;
    }
    result = 0;
done:
    /* EVP may emit unauthenticated plaintext before Final. It cannot escape
       this synchronous callback after any failure, including a bad tag. */
    if (result && dst->mv_size) OPENSSL_cleanse(dst->mv_data, dst->mv_size);
    if (result && enc) OPENSSL_cleanse(keys[2].mv_data, 16);
    OPENSSL_cleanse(final, sizeof(final));
    EVP_CIPHER_CTX_free(ctx);
    EVP_CIPHER_free(cipher);
    return result;
}
MODULE_EXPORT MDB_crypto_funcs *MDB_crypto(void) {
    /* Hook descriptors contain no key or context. Each call owns its EVP state. */
    static MDB_crypto_funcs hooks = {raw_hex_key, aes256gcm, NULL, 32, 16, 0};
    return &hooks;
}
