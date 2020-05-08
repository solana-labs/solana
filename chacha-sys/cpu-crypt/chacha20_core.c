#include "chacha.h"

#define ROTL32(v, n) (((v) << (n)) | ((v) >> (32 - (n))))

#define ROTATE(v, c) ROTL32((v), (c))

#define XOR(v, w) ((v) ^ (w))

#define PLUS(x, y) ((x) + (y))

#define U32TO8_LITTLE(p, v) \
{ (p)[0] = ((v)      ) & 0xff; (p)[1] = ((v) >>  8) & 0xff; \
  (p)[2] = ((v) >> 16) & 0xff; (p)[3] = ((v) >> 24) & 0xff; }

#define U8TO32_LITTLE(p)   \
     (((u32)((p)[0])      ) | ((u32)((p)[1]) <<  8) | \
      ((u32)((p)[2]) << 16) | ((u32)((p)[3]) << 24)   )

#define QUARTERROUND(a,b,c,d) \
  x[a] = PLUS(x[a],x[b]); x[d] = ROTATE(XOR(x[d],x[a]),16); \
  x[c] = PLUS(x[c],x[d]); x[b] = ROTATE(XOR(x[b],x[c]),12); \
  x[a] = PLUS(x[a],x[b]); x[d] = ROTATE(XOR(x[d],x[a]), 8); \
  x[c] = PLUS(x[c],x[d]); x[b] = ROTATE(XOR(x[b],x[c]), 7);

void chacha20_encrypt(const u32 input[16],
                      unsigned char output[64],
                      int num_rounds)
{
    u32 x[16];
    int i;
    memcpy(x, input, sizeof(u32) * 16);
    for (i = num_rounds; i > 0; i -= 2) {
        QUARTERROUND( 0, 4, 8,12)
        QUARTERROUND( 1, 5, 9,13)
        QUARTERROUND( 2, 6,10,14)
        QUARTERROUND( 3, 7,11,15)
        QUARTERROUND( 0, 5,10,15)
        QUARTERROUND( 1, 6,11,12)
        QUARTERROUND( 2, 7, 8,13)
        QUARTERROUND( 3, 4, 9,14)
    }
    for (i = 0; i < 16; ++i) {
        x[i] = PLUS(x[i], input[i]);
    }
    for (i = 0; i < 16; ++i) {
        U32TO8_LITTLE(output + 4 * i, x[i]);
    }
}

