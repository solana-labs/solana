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

#include "sha3.h"

//#include <avr/pgmspace.h>

#include <string.h>
#include <stdint.h>

#define BLOCK_SIZE     ((1600 - 256 * 2) / 8)

#define I64(x) x##LL
#define ROTL64(qword, n) ((qword) << (n) ^ ((qword) >> (64 - (n))))
#define le2me_64(x) (x)
#define IS_ALIGNED_64(p) (0 == (7 & ((const char*)(p) - (const char*)0)))
#define me64_to_le_str(to, from, length) memcpy((to), (from), (length))

/* constants */

//const uint8_t round_constant_info[] PROGMEM = {
//const uint8_t constants[] PROGMEM = {
const uint8_t constants[]  = {

    1, 26, 94, 112, 31, 33, 121, 85, 14, 12, 53, 38, 63, 79, 93, 83, 82, 72, 22, 102, 121, 88, 33, 116,
//};

//const uint8_t pi_transform[] PROGMEM = {
    1, 6, 9, 22, 14, 20, 2, 12, 13, 19, 23, 15, 4, 24, 21, 8, 16, 5, 3, 18, 17, 11, 7, 10,
//};

//const uint8_t rhoTransforms[] PROGMEM = {
    1, 62, 28, 27, 36, 44, 6, 55, 20, 3, 10, 43, 25, 39, 41, 45, 15, 21, 8, 18, 2, 61, 56, 14,
};

#define TYPE_ROUND_INFO      0
#define TYPE_PI_TRANSFORM   24
#define TYPE_RHO_TRANSFORM  48

uint8_t getConstant(uint8_t type, uint8_t index) {
    return constants[type + index];
    //return pgm_read_byte(&constants[type + index]);
}

static uint64_t get_round_constant(uint8_t round) {
    uint64_t result = 0;

    //uint8_t roundInfo = pgm_read_byte(&round_constant_info[round]);
    uint8_t roundInfo = getConstant(TYPE_ROUND_INFO, round);
    if (roundInfo & (1 << 6)) { result |= ((uint64_t)1 << 63); }
    if (roundInfo & (1 << 5)) { result |= ((uint64_t)1 << 31); }
    if (roundInfo & (1 << 4)) { result |= ((uint64_t)1 << 15); }
    if (roundInfo & (1 << 3)) { result |= ((uint64_t)1 << 7); }
    if (roundInfo & (1 << 2)) { result |= ((uint64_t)1 << 3); }
    if (roundInfo & (1 << 1)) { result |= ((uint64_t)1 << 1); }
    if (roundInfo & (1 << 0)) { result |= ((uint64_t)1 << 0); }

    return result;
}


/* Initializing a sha3 context for given number of output bits */
void keccak_init(SHA3_CTX *ctx) {
    /* NB: The Keccak capacity parameter = bits * 2 */

    memset(ctx, 0, sizeof(SHA3_CTX));
}

/* Keccak theta() transformation */
static void keccak_theta(uint64_t *A) {
    uint64_t C[5], D[5];

    for (uint8_t i = 0; i < 5; i++) {
        C[i] = A[i];
        for (uint8_t j = 5; j < 25; j += 5) { C[i] ^= A[i + j]; }
    }

    for (uint8_t i = 0; i < 5; i++) {
        D[i] = ROTL64(C[(i + 1) % 5], 1) ^ C[(i + 4) % 5];
    }

    for (uint8_t i = 0; i < 5; i++) {
        //for (uint8_t j = 0; j < 25; j += 5) {
        for (uint8_t j = 0; j < 25; j += 5) { A[i + j] ^= D[i]; }
    }
}


/* Keccak pi() transformation */
static void keccak_pi(uint64_t *A) {
    uint64_t A1 = A[1];
    //for (uint8_t i = 1; i < sizeof(pi_transform); i++) {
    for (uint8_t i = 1; i < 24; i++) {
        //A[pgm_read_byte(&pi_transform[i - 1])] = A[pgm_read_byte(&pi_transform[i])];
        A[getConstant(TYPE_PI_TRANSFORM, i - 1)] = A[getConstant(TYPE_PI_TRANSFORM, i)];
    }
    A[10] = A1;
    /* note: A[ 0] is left as is */
}

/*
ketch uses 30084 bytes (93%) of program storage space. Maximum is 32256 bytes.
Global variables use 743 bytes (36%) of dynamic memory, leaving 1305 bytes for local variables. Maximum is 2048 bytes.
*/
/* Keccak chi() transformation */
static void keccak_chi(uint64_t *A) {
    for (uint8_t i = 0; i < 25; i += 5) {
        uint64_t A0 = A[0 + i], A1 = A[1 + i];
        A[0 + i] ^= ~A1 & A[2 + i];
        A[1 + i] ^= ~A[2 + i] & A[3 + i];
        A[2 + i] ^= ~A[3 + i] & A[4 + i];
        A[3 + i] ^= ~A[4 + i] & A0;
        A[4 + i] ^= ~A0 & A1;
    }
}


static void sha3_permutation(uint64_t *state) {
    //for (uint8_t round = 0; round < sizeof(round_constant_info); round++) {
    for (uint8_t round = 0; round < 24; round++) {
        keccak_theta(state);

        /* apply Keccak rho() transformation */
        for (uint8_t i = 1; i < 25; i++) {
            //state[i] = ROTL64(state[i], pgm_read_byte(&rhoTransforms[i - 1]));
            state[i] = ROTL64(state[i], getConstant(TYPE_RHO_TRANSFORM, i - 1));
        }

        keccak_pi(state);
        keccak_chi(state);

        /* apply iota(state, round) */
        *state ^= get_round_constant(round);
    }
}

/**
 * The core transformation. Process the specified block of data.
 *
 * @param hash the algorithm state
 * @param block the message block to process
 * @param block_size the size of the processed block in bytes
 */
static void sha3_process_block(uint64_t hash[25], const uint64_t *block) {
    for (uint8_t i = 0; i < 17; i++) {
        hash[i] ^= le2me_64(block[i]);
    }

    /* make a permutation of the hash */
    sha3_permutation(hash);
}

//#define SHA3_FINALIZED 0x80000000
//#define SHA3_FINALIZED 0x8000

/**
 * Calculate message hash.
 * Can be called repeatedly with chunks of the message to be hashed.
 *
 * @param ctx the algorithm context containing current hashing state
 * @param msg message chunk
 * @param size length of the message chunk
 */
void keccak_update(SHA3_CTX *ctx, const unsigned char *msg, uint16_t size)
{
    uint16_t idx = (uint16_t)ctx->rest;

    //if (ctx->rest & SHA3_FINALIZED) return; /* too late for additional input */
    ctx->rest = (unsigned)((ctx->rest + size) % BLOCK_SIZE);

    /* fill partial block */
    if (idx) {
        uint16_t left = BLOCK_SIZE - idx;
        memcpy((char*)ctx->message + idx, msg, (size < left ? size : left));
        if (size < left) return;

        /* process partial block */
        sha3_process_block(ctx->hash, ctx->message);
        msg  += left;
        size -= left;
    }

    while (size >= BLOCK_SIZE) {
        uint64_t* aligned_message_block;
        if (IS_ALIGNED_64(msg)) {
            // the most common case is processing of an already aligned message without copying it
            aligned_message_block = (uint64_t*)(void*)msg;
        } else {
            memcpy(ctx->message, msg, BLOCK_SIZE);
            aligned_message_block = ctx->message;
        }

        sha3_process_block(ctx->hash, aligned_message_block);
        msg  += BLOCK_SIZE;
        size -= BLOCK_SIZE;
    }

    if (size) {
        memcpy(ctx->message, msg, size); /* save leftovers */
    }
}

/**
* Store calculated hash into the given array.
*
* @param ctx the algorithm context containing current hashing state
* @param result calculated hash in binary form
*/
void keccak_final(SHA3_CTX *ctx, unsigned char* result)
{
    uint16_t digest_length = 100 - BLOCK_SIZE / 2;

//    if (!(ctx->rest & SHA3_FINALIZED)) {
        /* clear the rest of the data queue */
        memset((char*)ctx->message + ctx->rest, 0, BLOCK_SIZE - ctx->rest);
        ((char*)ctx->message)[ctx->rest] |= 0x01;
        ((char*)ctx->message)[BLOCK_SIZE - 1] |= 0x80;

        /* process final block */
        sha3_process_block(ctx->hash, ctx->message);
//        ctx->rest = SHA3_FINALIZED; /* mark context as finalized */
//    }

    if (result) {
         me64_to_le_str(result, ctx->hash, digest_length);
    }
}

void keccak256(const unsigned char *msg, uint16_t size, unsigned char* result) {
    inplace_reverse((char*)msg, size);

    SHA3_CTX ctx;
    keccak_init(&ctx);
    keccak_update(&ctx, msg, size);
    keccak_final(&ctx, result);

    inplace_reverse((char*)result, 32);
}