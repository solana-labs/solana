/*
 * Adapted from kste's sha256 implementation, accessible at https://github.com/kste/sha256_avx
 *
 * MIT License
 *
 * Copyright (c) 2017
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */

#ifndef SHA256_H
#define SHA256_H

#define u32 unsigned int32
#define u8 unsigned int8

#define SHA256_BLOCK_SIZE 32

static const u32 RC[] = {
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2
};

#define MAJ(a, b, c) ((a&b) ^ (a&c) ^ (b&c))
#define CH(a, b, c) ((a&b) ^ (~(a)&c))

#define ROTR32(x, r) ((x >> r) | (x << (SHA256_BLOCK_SIZE - r)))

#define SIGMA1(x) (ROTR32(x, 6) ^ ROTR32(x, 11) ^ ROTR32(x, 25))
#define SIGMA0(x) (ROTR32(x, 2) ^ ROTR32(x, 13) ^ ROTR32(x, 22))

#define WSIGMA1(x) (ROTR32(x, 17) ^ ROTR32(x, 19) ^ (x >> 10))
#define WSIGMA0(x) (ROTR32(x, 7) ^ ROTR32(x, 18) ^ (x >> 3))

#define SHA256ROUND(a, b, c, d, e, f, g, h, rc, w) \
    T0 = h + SIGMA1(e) + CH(e, f, g) + RC[rc] + w; \
    d = d + T0; \
    T1 = SIGMA0(a) + MAJ(a, b, c); \
    h = T0 + T1;

#endif