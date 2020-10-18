/* sha3 - an implementation of Secure Hash Algorithm 3 (Keccak).
 * based on the
 * The Keccak SHA-3 submission. Submission to NIST (Round 3), 2011
 * by Guido Bertoni, Joan Daemen, Michaël Peeters and Gilles Van Assche
 *
 * Copyright: 2013 Aleksey Kravchenko <rhash.admin@gmail.com>
 *
 * Permission is hereby granted,  free of charge,  to any person  obtaining a
 * copy of this software and associated documentation files (the "Software"),
 * to deal in the Software without restriction,  including without limitation
 * the rights to  use, copy, modify,  merge, publish, distribute, sublicense,
 * and/or sell copies  of  the Software,  and to permit  persons  to whom the
 * Software is furnished to do so.
 *
 * This program  is  distributed  in  the  hope  that it will be useful,  but
 * WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
 * or FITNESS FOR A PARTICULAR PURPOSE.  Use this program  at  your own risk!
 */

#ifndef __KECCAK256_H_
#define __KECCAK256_H_

#include <stdint.h>
#include "utils.h"

#define sha3_max_permutation_size 25
#define sha3_max_rate_in_qwords 24

typedef struct SHA3_CTX {
    /* 1600 bits algorithm hashing state */
    uint64_t hash[sha3_max_permutation_size];
    /* 1536-bit buffer for leftovers */
    uint64_t message[sha3_max_rate_in_qwords];
    /* count of bytes in the message[] buffer */
    uint16_t rest;
    /* size of a message block processed at once */
    //unsigned block_size;
} SHA3_CTX;


#ifdef __cplusplus
extern "C" {
#endif  /* __cplusplus */


void keccak_init(SHA3_CTX *ctx);
void keccak_update(SHA3_CTX *ctx, const unsigned char *msg, uint16_t size);
void keccak_final(SHA3_CTX *ctx, unsigned char* result);

void keccak256(const unsigned char *msg, uint16_t size, unsigned char* result);


#ifdef __cplusplus
}
#endif  /* __cplusplus */

#endif  /* __KECCAK256_H_ */