#pragma once
/**
 * @brief Solana bn128 elliptic curve compression and decompression
**/

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Output length for the g1 compress operation.
 */
#define ALT_BN128_COMPRESSION_G1_COMPRESS_OUTPUT_LEN 32

/**
 * Output length for the g1 decompress operation.
 */
#define ALT_BN128_COMPRESSION_G1_DECOMPRESS_OUTPUT_LEN 64

/**
 * Output length for the g1 compress operation.
 */
#define ALT_BN128_COMPRESSION_G2_COMPRESS_OUTPUT_LEN 64

/**
 * Output length for the g2 decompress operation.
 */
#define ALT_BN128_COMPRESSION_G2_DECOMPRESS_OUTPUT_LEN 128

/**
 * G1 compression operation.
 */
#define ALT_BN128_G1_COMPRESS 0

/**
 * G1 decompression operation.
 */
#define ALT_BN128_G1_DECOMPRESS 1

/**
 * G2 compression operation.
 */
#define ALT_BN128_G2_COMPRESS 2

/**
 * G2 decompression operation.
 */
#define ALT_BN128_G2_DECOMPRESS 3

/**
 * Compression of alt_bn128 g1 and g2 points
 *
 * @param op ...
 * @param input ...
 * @param input_size ...
 * @param result 64 byte array to hold the result. ...
 * @return 0 if executed successfully
 */
@SYSCALL uint64_t sol_alt_bn128_compression(
        const uint64_t op,
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);

#ifdef __cplusplus
}
#endif

/**@}*/
