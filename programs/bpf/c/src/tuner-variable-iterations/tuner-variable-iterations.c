/**
 * @brief Compute budget tuner program.  Spins in a loop for the specified number of iterations
 * (or for UINT64_MAX iterations if 0 is specified for the number of iterations), in order to consume
 * a configurable amount of the budget.
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
  iterations = iterations == 0 ? UINT64_MAX : iterations;
  size_t rand = params.data[1];
  size_t account_index = rand % params.ka_num;
  uint8_t *val = (uint8_t *)ka[account_index].data;
  uint64_t memory_size = ka[account_index].data_len;

  for (uint64_t i = 0; i < iterations; i++) {
    {
      *val ^= val[current % memory_size];
      current = current * 76510171 + 47123;
    }
  }
  return *val;
}
