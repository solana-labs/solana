/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */
#include <sand/types.h>
#include <sand/log.h>
#include <sand/deserialize_deprecated.h>

extern uint64_t entrypoint(const uint8_t *input) {
  SolAccountInfo ka[1];
  SolParameters params = (SolParameters) { .ka = ka };

  sand_log(__FILE__);

  if (!sand_deserialize_deprecated(input, &params, SAND_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }

  // Log the provided input parameters.  In the case of  the no-op
  // program, no account keys or input data are expected but real
  // programs will have specific requirements so they can do their work.
  sand_log_params(&params);
  return SUCCESS;
}
