/**
 * @brief Example C based SBF program that prints out the parameters
 * passed to it
 */

#include <solana_sdk.h>
#include <sol/deserialize_deprecated.h>

extern uint64_t entrypoint(const uint8_t *input) {
  SolAccountInfo ka[2];
  SolParameters params = (SolParameters) { .ka = ka };

  if (!sol_deserialize_deprecated(input, &params, SOL_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }

  return SUCCESS;
}
