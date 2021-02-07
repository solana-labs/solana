/**
 * @brief SHA256 Syscall test
 */
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {

  uint8_t result[SHA256_RESULT_LENGTH];
  uint8_t expected[] = {0x9f, 0xa2, 0x7e, 0x8f, 0x7b, 0xc1, 0xec, 0xe8,
                        0xae, 0x7b, 0x9a, 0x91, 0x46, 0x53, 0x20, 0xf,
                        0x1c, 0x22, 0x8e, 0x56, 0x10, 0x30, 0x59, 0xfd,
                        0x35, 0x8d, 0x57, 0x54, 0x96, 0x47, 0x2c, 0xc9};

  uint8_t bytes1[] = {'G', 'a', 'g', 'g', 'a', 'b', 'l', 'a',
                      'g', 'h', 'b', 'l', 'a', 'g', 'h', '!'};
  uint8_t bytes2[] = {'f', 'l', 'u', 'r', 'b', 'o', 's'};
  const SolBytes bytes[] = {{bytes1, SOL_ARRAY_SIZE(bytes1)},
                            {bytes2, SOL_ARRAY_SIZE(bytes2)}};

  sol_sha256(bytes, SOL_ARRAY_SIZE(bytes), result);

  sol_assert(0 == sol_memcmp(result, expected, SHA256_RESULT_LENGTH));

  return SUCCESS;
}
