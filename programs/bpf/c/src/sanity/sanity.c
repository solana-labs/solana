/**
 * @brief Example C-based BPF sanity rogram that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  SafeAccountInfo ka[1];
  SafeParameters params = (SafeParameters) { .ka = ka };

  sol_log(__FILE__);

  if (!sol_deserialize(input, &params, SAFE_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }

  // Log the provided input parameters.  In the case of  the no-op
  // program, no account keys or input data are expected but real
  // programs will have specific requirements so they can do their work.
  sol_log_params(&params);

  sol_log_compute_units();
  return SUCCESS;
}
