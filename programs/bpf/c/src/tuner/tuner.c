/**
 * @brief Compute budget tuner program.  Spins in a loop consuming the entire
 * budget, used by the tuner bench test to tune the compute budget costs.
 *
 * Care should be taken because the compiler might optimize out the mechanism
 * you are trying to tune.
 */

#include <solana_sdk.h>

#define NUM_KA 1

extern uint64_t entrypoint(const uint8_t *input) {
  SafeAccountInfo ka[NUM_KA];
  SafeParameters params = (SafeParameters){.ka = ka};
  if (!sol_deserialize(input, &params, SAFE_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }
  uint8_t *val = (uint8_t *)ka[0].data;
  size_t current = 1;
  for (uint64_t i = 0; i < UINT64_MAX; i++) {

    // Uncomment for raw compute
    {
      *val ^= val[current % 10000001] + 13181312;
      current *= 12345678;
    }

    // // Uncomment for SHA256 syscall
    // {
    //   uint8_t result[SHA256_RESULT_LENGTH];
    //   uint8_t bytes1[1024];
    //   const SafeBytes bytes[] = {{bytes1, SAFE_ARRAY_SIZE(bytes1)}};

    //   sol_sha256(bytes, SAFE_ARRAY_SIZE(bytes), result);
    //   *val = result[0];
    // }

    // // Uncomment for Pubkey logging syscall
    // {
    //   SafePubkey pubkey;
    //   sol_log_pubkey(&pubkey);
    // }
  }
  return *val;
}
