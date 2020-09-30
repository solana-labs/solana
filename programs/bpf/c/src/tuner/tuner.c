/**
 * @brief Compute budget tuner program.  Spins in a loop consuming the entire
 * budget, used by the tuner bench test to tune the compute budget costs.
 *
 * Care should be taken because the compiler might optimize out the mechanism
 * you are trying to tune.
 */

#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  uint8_t *val = (uint8_t *)input;
  for (uint64_t i = 0; i < UINT64_MAX; i++) {

    // Uncomment for raw compute
    {
      if (*val != 0) {
        *val = *val + 1;
      }
    }

    // // Uncomment for SHA256 syscall
    // {
    //   uint8_t result[SHA256_RESULT_LENGTH];
    //   uint8_t bytes1[1024];
    //   const SolBytes bytes[] = {{bytes1, SOL_ARRAY_SIZE(bytes1)}};

    //   sol_sha256(bytes, SOL_ARRAY_SIZE(bytes), result);
    //   *val = result[0];
    // }
  }
  return *val;
}
