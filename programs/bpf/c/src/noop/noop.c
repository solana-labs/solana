/**
 * @brief Example C-based BPF noop program
 */
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  SafeAccountInfo ka[1];
  SafeParameters params = (SafeParameters) { .ka = ka };
  if (!sol_deserialize(input, &params, SAFE_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }
  return SUCCESS;
}
