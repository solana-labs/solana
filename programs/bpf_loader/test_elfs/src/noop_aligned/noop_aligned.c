/**
 * @brief Example C based SBF program that prints out the parameters
 * passed to it
 */
#include <sol/deserialize.h>


extern uint64_t entrypoint(const uint8_t *input) {
  SolAccountInfo ka[2];
  SolParameters params = (SolParameters) { .ka = ka };

  if (!sol_deserialize(input, &params, SOL_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }

  return SUCCESS;
}
