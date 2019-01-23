/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>

extern bool entrypoint(const uint8_t *input) {
  SolKeyedAccount ka[1];
  SolParameters params = (SolParameters) { .ka = ka };

  sol_log(__FILE__);

  if (!sol_deserialize(input, &params, SOL_ARRAY_SIZE(ka))) {
    return false;
  }

  // Log the provided input parameters.  In the case of  the no-op
  // program, no account keys or input data are expected but real
  // programs will have specific requirements so they can do their work.
  sol_log_params(&params);
  return true;
}

