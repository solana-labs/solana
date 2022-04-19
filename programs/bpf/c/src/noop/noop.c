/**
 * @brief Example C based BPF program that prints out the parameters
 * passed to it
 */
#include <sand/deserialize.h>


extern uint64_t entrypoint(const uint8_t *input) {
  SolAccountInfo ka[1];
  SolParameters params = (SolParameters) { .ka = ka };

  if (!sand_deserialize(input, &params, SAND_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }

  return SUCCESS;
}
