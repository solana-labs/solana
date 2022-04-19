/**
 * @brief Example C-based BPF program that exercises duplicate keyed accounts
 * passed to it
 */
#include <solana_sdk.h>

/**
 * Custom error for when input serialization fails
 */

extern uint64_t entrypoint(const uint8_t *input) {
  SolAccountInfo accounts[5];
  SolParameters params = (SolParameters){.ka = accounts};

  if (!sand_deserialize(input, &params, SAND_ARRAY_SIZE(accounts))) {
    return ERROR_INVALID_ARGUMENT;
  }

  switch (params.data[0]) {
  case (1):
    sand_log("modify first account data");
    accounts[2].data[0] = 1;
    break;
  case (2):
    sand_log("modify first account data");
    accounts[3].data[0] = 2;
    break;
  case (3):
    sand_log("modify both account data");
    accounts[2].data[0] += 1;
    accounts[3].data[0] += 2;
    break;
  case (4):
    sand_log("modify first account lamports");
    *accounts[1].lamports -= 1;
    *accounts[2].lamports += 1;
    break;
  case (5):
    sand_log("modify first account lamports");
    *accounts[1].lamports -= 2;
    *accounts[3].lamports += 2;
    break;
  case (6):
    sand_log("modify both account lamports");
    *accounts[1].lamports -= 3;
    *accounts[2].lamports += 1;
    *accounts[3].lamports += 2;
    break;
  case (7):
    sand_log("check account (0,1,2,3) privs");
    sand_assert(accounts[0].is_signer);
    sand_assert(!accounts[1].is_signer);
    sand_assert(accounts[2].is_signer);
    sand_assert(accounts[3].is_signer);

    sand_assert(accounts[0].is_writable);
    sand_assert(accounts[1].is_writable);
    sand_assert(accounts[2].is_writable);
    sand_assert(accounts[3].is_writable);

    if (params.ka_num > 4) {
      {
        SolAccountMeta arguments[] = {{accounts[0].key, true, true},
                                      {accounts[1].key, true, false},
                                      {accounts[2].key, true, false},
                                      {accounts[3].key, false, true}};
        uint8_t data[] = {7};
        const SolInstruction instruction = {
            (SolPubkey *)params.program_id, arguments,
            SAND_ARRAY_SIZE(arguments), data, SAND_ARRAY_SIZE(data)};
        sand_assert(SUCCESS ==
                   sand_invoke(&instruction, accounts, params.ka_num));
      }
      {
        SolAccountMeta arguments[] = {{accounts[0].key, true, true},
                                      {accounts[1].key, true, false},
                                      {accounts[2].key, true, false},
                                      {accounts[3].key, true, false}};
        uint8_t data[] = {3};
        const SolInstruction instruction = {
            (SolPubkey *)params.program_id, arguments,
            SAND_ARRAY_SIZE(arguments), data, SAND_ARRAY_SIZE(data)};
        sand_assert(SUCCESS ==
                   sand_invoke(&instruction, accounts, params.ka_num));
      }
      sand_assert(accounts[2].data[0] == 3);
      sand_assert(accounts[3].data[0] == 3);
    }
    break;
  default:
    sand_log("Unrecognized command");
    return ERROR_INVALID_INSTRUCTION_DATA;
  }
  return SUCCESS;
}
