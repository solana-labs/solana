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
  SolAccountInfo ka[NUM_KA];
  SolParameters params = (SolParameters){.ka = ka};
  if (!sol_deserialize(input, &params, SOL_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }
  size_t current = 1;
  uint64_t iterations = params.data[0] * 18;
  size_t rand = params.data[1];
  size_t account_index = rand % params.ka_num;
  uint8_t *val = (uint8_t *)ka[account_index].data;
  uint64_t memory_size = ka[account_index].data_len;

  for (uint64_t i = 0; i < iterations; i++) {

    // Uncomment for raw compute // 142 iterations ~= 200k
    {
      /*if (i % 20 == 0) { // 83 iterations with this
        sol_log_64(memory_size, current, i, iterations, 0);
      }*/
      *val ^= val[current % memory_size];
      current = current * 76510171 + 47123;
    }

    // // Uncomment for SHA256 syscall
    // {
    //   uint8_t result[SHA256_RESULT_LENGTH];
    //   uint8_t bytes1[1024];
    //   const SolBytes bytes[] = {{bytes1, SOL_ARRAY_SIZE(bytes1)}};

    //   sol_sha256(bytes, SOL_ARRAY_SIZE(bytes), result);
    //   *val = result[0];
    // }

    // // Uncomment for Pubkey logging syscall
    // {
    //   SolPubkey pubkey;
    //   sol_log_pubkey(&pubkey);
    // }
  }
  return *val;
}
