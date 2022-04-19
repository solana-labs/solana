/**
 * @brief Example C++ based BPF program that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  SandAccountInfo ka[1];
  SandParameters params = (SandParameters) { .ka = ka };

  if (!sand_deserialize(input, &params, SAND_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }

  return SUCCESS;
}
