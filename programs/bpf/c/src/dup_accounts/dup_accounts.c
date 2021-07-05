/**
 * @brief Example C-based BPF program that exercises duplicate keyed accounts
 * passed to it
 */
#include <solana_sdk.h>

/**
 * Custom error for when input serialization fails
 */

extern uint64_t entrypoint(const uint8_t *input) {
  SafeAccountInfo accounts[5];
  SafeParameters params = (SafeParameters){.ka = accounts};

  if (!sol_deserialize(input, &params, SAFE_ARRAY_SIZE(accounts))) {
    return ERROR_INVALID_ARGUMENT;
  }

  switch (params.data[0]) {
  case (1):
    sol_log("modify first account data");
    accounts[2].data[0] = 1;
    break;
  case (2):
    sol_log("modify first account data");
    accounts[3].data[0] = 2;
    break;
  case (3):
    sol_log("modify both account data");
    accounts[2].data[0] += 1;
    accounts[3].data[0] += 2;
    break;
  case (4):
    sol_log("modify first account lamports");
    *accounts[1].lamports -= 1;
    *accounts[2].lamports += 1;
    break;
  case (5):
    sol_log("modify first account lamports");
    *accounts[1].lamports -= 2;
    *accounts[3].lamports += 2;
    break;
  case (6):
    sol_log("modify both account lamports");
    *accounts[1].lamports -= 3;
    *accounts[2].lamports += 1;
    *accounts[3].lamports += 2;
    break;
  case (7):
    sol_log("check account (0,1,2,3) privs");
    sol_assert(accounts[0].is_signer);
    sol_assert(!accounts[1].is_signer);
    sol_assert(accounts[2].is_signer);
    sol_assert(accounts[3].is_signer);

    sol_assert(accounts[0].is_writable);
    sol_assert(accounts[1].is_writable);
    sol_assert(accounts[2].is_writable);
    sol_assert(accounts[3].is_writable);

    if (params.ka_num > 4) {
      {
        SafeAccountMeta arguments[] = {{accounts[0].key, true, true},
                                      {accounts[1].key, true, false},
                                      {accounts[2].key, true, false},
                                      {accounts[3].key, false, true}};
        uint8_t data[] = {7};
        const SafeInstruction instruction = {
            (SafePubkey *)params.program_id, arguments,
            SAFE_ARRAY_SIZE(arguments), data, SAFE_ARRAY_SIZE(data)};
        sol_assert(SUCCESS ==
                   sol_invoke(&instruction, accounts, params.ka_num));
      }
      {
        SafeAccountMeta arguments[] = {{accounts[0].key, true, true},
                                      {accounts[1].key, true, false},
                                      {accounts[2].key, true, false},
                                      {accounts[3].key, true, false}};
        uint8_t data[] = {3};
        const SafeInstruction instruction = {
            (SafePubkey *)params.program_id, arguments,
            SAFE_ARRAY_SIZE(arguments), data, SAFE_ARRAY_SIZE(data)};
        sol_assert(SUCCESS ==
                   sol_invoke(&instruction, accounts, params.ka_num));
      }
      sol_assert(accounts[2].data[0] == 3);
      sol_assert(accounts[3].data[0] == 3);
    }
    break;
  default:
    sol_log("Unrecognized command");
    return ERROR_INVALID_INSTRUCTION_DATA;
  }
  return SUCCESS;
}
