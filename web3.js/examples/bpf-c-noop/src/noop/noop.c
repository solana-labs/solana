/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */

#include <solana_sdk.h>

extern uint32_t entrypoint(const uint8_t *input) {
  SolKeyedAccount ka[1];
  SolParameters params = (SolParameters) { .ka = ka };

  sol_log("Hello World");

  if (!sol_deserialize(input, &params, SOL_ARRAY_SIZE(ka))) {
    return 1;
  }
  sol_log_params(&params);
  return SUCCESS;
}
