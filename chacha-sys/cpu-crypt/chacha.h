#ifndef HEADER_CHACHA_H
# define HEADER_CHACHA_H

#include <string.h>
#include <inttypes.h>
# include <stddef.h>
# ifdef  __cplusplus
extern "C" {
# endif

typedef unsigned int u32;

#define CHACHA_KEY_SIZE 32
#define CHACHA_NONCE_SIZE 12
#define CHACHA_BLOCK_SIZE 64
#define CHACHA_ROUNDS 500

void chacha20_encrypt(const u32 input[16],
                      unsigned char output[64],
                      int num_rounds);

void chacha20_cbc128_encrypt(const unsigned char* in, unsigned char* out,
                             uint32_t len, const uint8_t* key,
                             unsigned char* ivec);


# ifdef  __cplusplus
}
# endif

#endif
