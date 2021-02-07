/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */

#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  SafeAccountInfo ka[1];
  SafeParameters params = (SafeParameters) { .ka = ka };

  sol_log("Hello World");

  if (!sol_deserialize(input, &params, SAFE_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }
  sol_log_params(&params);
  return SUCCESS;
}
