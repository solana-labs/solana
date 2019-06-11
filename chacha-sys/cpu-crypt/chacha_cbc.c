#include "chacha.h"

#if !defined(STRICT_ALIGNMENT) && !defined(PEDANTIC)
# define STRICT_ALIGNMENT 0
#endif

void chacha20_cbc128_encrypt(const unsigned char* in, unsigned char* out,
                             uint32_t len, const uint8_t* key,
                             unsigned char* ivec)
{
    size_t n;
    unsigned char *iv = ivec;
    (void)key;

    if (len == 0) {
        return;
    }

#if !defined(OPENSSL_SMALL_FOOTPRINT)
    if (STRICT_ALIGNMENT &&
        ((size_t)in | (size_t)out | (size_t)ivec) % sizeof(size_t) != 0) {
        while (len >= CHACHA_BLOCK_SIZE) {
            for (n = 0; n < CHACHA_BLOCK_SIZE; ++n) {
                out[n] = in[n] ^ iv[n];
                //printf("%x ", out[n]);
            }
            chacha20_encrypt((const u32*)out, out, CHACHA_ROUNDS);
            iv = out;
            len -= CHACHA_BLOCK_SIZE;
            in += CHACHA_BLOCK_SIZE;
            out += CHACHA_BLOCK_SIZE;
        }
    } else {
        while (len >= CHACHA_BLOCK_SIZE) {
            for (n = 0; n < CHACHA_BLOCK_SIZE; n += sizeof(size_t)) {
                *(size_t *)(out + n) =
                    *(size_t *)(in + n) ^ *(size_t *)(iv + n);
                //printf("%zu ", *(size_t *)(iv + n));
            }
            chacha20_encrypt((const u32*)out, out, CHACHA_ROUNDS);
            iv = out;
            len -= CHACHA_BLOCK_SIZE;
            in += CHACHA_BLOCK_SIZE;
            out += CHACHA_BLOCK_SIZE;
        }
    }
#endif
    while (len) {
        for (n = 0; n < CHACHA_BLOCK_SIZE && n < len; ++n) {
            out[n] = in[n] ^ iv[n];
        }
        for (; n < CHACHA_BLOCK_SIZE; ++n) {
            out[n] = iv[n];
        }
        chacha20_encrypt((const u32*)out, out, CHACHA_ROUNDS);
        iv = out;
        if (len <= CHACHA_BLOCK_SIZE) {
            break;
        }
        len -= CHACHA_BLOCK_SIZE;
        in += CHACHA_BLOCK_SIZE;
        out += CHACHA_BLOCK_SIZE;
    }
    memcpy(ivec, iv, CHACHA_BLOCK_SIZE);

}

void chacha20_cbc_encrypt(const uint8_t *in, uint8_t *out, size_t in_len,
                          const uint8_t key[CHACHA_KEY_SIZE], uint8_t* ivec)
{
    chacha20_cbc128_encrypt(in, out, in_len, key, ivec);
}
