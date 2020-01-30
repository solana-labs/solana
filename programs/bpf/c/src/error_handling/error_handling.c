/**
 * @brief Example C-based BPF program that exercises duplicate keyed ka
 * passed to it
 */
#include <solana_sdk.h>

/**
 * Custom error for when input serialization fails
 */

extern uint32_t entrypoint(const uint8_t *input) {
  SolKeyedAccount ka[4];
  SolParameters params = (SolParameters) { .ka = ka };

  if (!sol_deserialize(input, &params, SOL_ARRAY_SIZE(ka))) {
    return INVALID_ARGUMENT;
  }

  switch (params.data[0]) {
    case(1):
        sol_log("return success");
        return SUCCESS;
    case(2):
        sol_log("return a builtin");
        return INVALID_ACCOUNT_DATA;
    case(3):
        sol_log("return custom error");
        return 42;
    case(4):
        sol_log("return error that conflicts with success");
        return 0x40000000;
    default:
        sol_log("Unrecognized command");
        return INVALID_INSTRUCTION_DATA;
  }
  return SUCCESS;
}
