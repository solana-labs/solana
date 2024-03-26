/**
 * @brief alt_bn128 syscall test
 */
#include <sol/alt_bn128_compression.h>
#include <sol/assert.h>
#include <sol/string.h>

extern uint64_t entrypoint(const uint8_t *input) {
  // compress and decompress g1
  {
    uint8_t result_compressed[ALT_BN128_COMPRESSION_G1_COMPRESS_OUTPUT_LEN];
    uint8_t result_decompressed[ALT_BN128_COMPRESSION_G1_DECOMPRESS_OUTPUT_LEN];
    uint8_t input[] = {
            45, 206, 255, 166, 152, 55, 128, 138, 79, 217, 145, 164, 25, 74, 120, 234, 234, 217,
            68, 149, 162, 44, 133, 120, 184, 205, 12, 44, 175, 98, 168, 172, 20, 24, 216, 15, 209,
            175, 106, 75, 147, 236, 90, 101, 123, 219, 245, 151, 209, 202, 218, 104, 148, 8, 32,
            254, 243, 191, 218, 122, 42, 81, 193, 84,
    };

    sol_alt_bn128_compression(ALT_BN128_G1_COMPRESS, input, SOL_ARRAY_SIZE(input), result_compressed);
    sol_alt_bn128_compression(ALT_BN128_G1_DECOMPRESS, result_compressed, SOL_ARRAY_SIZE(result_compressed), result_decompressed);

    sol_assert(0 ==
               sol_memcmp(result_decompressed, input, ALT_BN128_COMPRESSION_G1_DECOMPRESS_OUTPUT_LEN));
  }

  // compress and decompress g2
 {

    uint8_t result_compressed[ALT_BN128_COMPRESSION_G2_COMPRESS_OUTPUT_LEN];
    uint8_t result_decompressed[ALT_BN128_COMPRESSION_G2_DECOMPRESS_OUTPUT_LEN];
    uint8_t input[] = {
            40, 57, 233, 205, 180, 46, 35, 111, 215, 5, 23, 93, 12, 71, 118, 225, 7, 46, 247, 147,
            47, 130, 106, 189, 184, 80, 146, 103, 141, 52, 242, 25, 0, 203, 124, 176, 110, 34, 151,
            212, 66, 180, 238, 151, 236, 189, 133, 209, 17, 137, 205, 183, 168, 196, 92, 159, 75,
            174, 81, 168, 18, 86, 176, 56, 16, 26, 210, 20, 18, 81, 122, 142, 104, 62, 251, 169,
            98, 141, 21, 253, 50, 130, 182, 15, 33, 109, 228, 31, 79, 183, 88, 147, 174, 108, 4,
            22, 14, 129, 168, 6, 80, 246, 254, 100, 218, 131, 94, 49, 247, 211, 3, 245, 22, 200,
            177, 91, 60, 144, 147, 174, 90, 17, 19, 189, 62, 147, 152, 18,
        };

    sol_alt_bn128_compression(ALT_BN128_G2_COMPRESS, input, SOL_ARRAY_SIZE(input), result_compressed);
    sol_alt_bn128_compression(ALT_BN128_G2_DECOMPRESS, result_compressed, SOL_ARRAY_SIZE(result_compressed), result_decompressed);

    sol_assert(
        0 == sol_memcmp(result_decompressed, input, ALT_BN128_COMPRESSION_G2_DECOMPRESS_OUTPUT_LEN));
  }

  return SUCCESS;
}
