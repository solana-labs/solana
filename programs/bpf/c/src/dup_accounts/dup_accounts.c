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
        sol_log("modify first account userdata");
        ka[2].userdata[0] = 1;
        break;
    case(2):
        sol_log("modify first account userdata");
        ka[3].userdata[0] = 2;
        break;
    case(3):
        sol_log("modify both account userdata");
        ka[2].userdata[0] += 1;
        ka[3].userdata[0] += 2;
        break;
    case(4):
        sol_log("modify first account lamports");
        *ka[1].lamports -= 1;
        *ka[2].lamports += 1;
        break;
    case(5):
        sol_log("modify first account lamports");
        *ka[1].lamports -= 2;
        *ka[3].lamports += 2;
        break;
    case(6):
        sol_log("modify both account lamports");
        *ka[1].lamports -= 3;
        *ka[2].lamports += 1;
        *ka[3].lamports += 2;
        break;
    default:
        sol_log("Unrecognized command");
        return INVALID_INSTRUCTION_DATA;
  }
  return SUCCESS;
}
