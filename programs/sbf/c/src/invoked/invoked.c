/**
 * @brief Example C-based SBF program that tests cross-program invocations
 */
#include "instruction.h"
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  sol_log("Invoked C program");

  SolAccountInfo accounts[4];
  SolParameters params = (SolParameters){.ka = accounts};

  if (!sol_deserialize(input, &params, 0)) {
    return ERROR_INVALID_ARGUMENT;
  }

  // on entry, return data must not be set
  sol_assert(sol_get_return_data(NULL, 0, NULL) == 0);

  if (params.data_len == 0) {
    return SUCCESS;
  }

  switch (params.data[0]) {
  case VERIFY_TRANSLATIONS: {
    sol_log("verify data translations");

    static const int ARGUMENT_INDEX = 0;
    static const int INVOKED_ARGUMENT_INDEX = 1;
    static const int INVOKED_PROGRAM_INDEX = 2;
    static const int INVOKED_PROGRAM_DUP_INDEX = 3;
    sol_assert(sol_deserialize(input, &params, 4));

    SolPubkey sbf_loader_id =
        (SolPubkey){.x = {2,  168, 246, 145, 78,  136, 161, 110, 57,  90, 225,
                          40, 148, 143, 250, 105, 86,  147, 55,  104, 24, 221,
                          71, 67,  82,  33,  243, 198, 0,   0,   0,   0}};

    for (int i = 0; i < params.data_len; i++) {
      sol_assert(params.data[i] == i);
    }
    sol_assert(params.ka_num == 4);

    sol_assert(*accounts[ARGUMENT_INDEX].lamports == 42);
    sol_assert(accounts[ARGUMENT_INDEX].data_len == 100);
    sol_assert(accounts[ARGUMENT_INDEX].is_signer);
    sol_assert(accounts[ARGUMENT_INDEX].is_writable);
    sol_assert(accounts[ARGUMENT_INDEX].rent_epoch == UINT64_MAX);
    sol_assert(!accounts[ARGUMENT_INDEX].executable);
    for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
      sol_assert(accounts[ARGUMENT_INDEX].data[i] == i);
    }

    sol_assert(SolPubkey_same(accounts[INVOKED_ARGUMENT_INDEX].owner,
                              accounts[INVOKED_PROGRAM_INDEX].key));
    sol_assert(*accounts[INVOKED_ARGUMENT_INDEX].lamports == 10);
    sol_assert(accounts[INVOKED_ARGUMENT_INDEX].data_len == 10);
    sol_assert(accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    sol_assert(accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    sol_assert(accounts[INVOKED_ARGUMENT_INDEX].rent_epoch == UINT64_MAX);
    sol_assert(!accounts[INVOKED_ARGUMENT_INDEX].executable);

    sol_assert(
        SolPubkey_same(accounts[INVOKED_PROGRAM_INDEX].key, params.program_id))
        sol_assert(SolPubkey_same(accounts[INVOKED_PROGRAM_INDEX].owner,
                                  &sbf_loader_id));
    sol_assert(!accounts[INVOKED_PROGRAM_INDEX].is_signer);
    sol_assert(!accounts[INVOKED_PROGRAM_INDEX].is_writable);
    sol_assert(accounts[INVOKED_PROGRAM_INDEX].rent_epoch == UINT64_MAX);
    sol_assert(accounts[INVOKED_PROGRAM_INDEX].executable);

    sol_assert(SolPubkey_same(accounts[INVOKED_PROGRAM_INDEX].key,
                              accounts[INVOKED_PROGRAM_DUP_INDEX].key));
    sol_assert(SolPubkey_same(accounts[INVOKED_PROGRAM_INDEX].owner,
                              accounts[INVOKED_PROGRAM_DUP_INDEX].owner));
    sol_assert(*accounts[INVOKED_PROGRAM_INDEX].lamports ==
               *accounts[INVOKED_PROGRAM_DUP_INDEX].lamports);
    sol_assert(accounts[INVOKED_PROGRAM_INDEX].is_signer ==
               accounts[INVOKED_PROGRAM_DUP_INDEX].is_signer);
    sol_assert(accounts[INVOKED_PROGRAM_INDEX].is_writable ==
               accounts[INVOKED_PROGRAM_DUP_INDEX].is_writable);
    sol_assert(accounts[INVOKED_PROGRAM_INDEX].rent_epoch ==
               accounts[INVOKED_PROGRAM_DUP_INDEX].rent_epoch);
    sol_assert(accounts[INVOKED_PROGRAM_INDEX].executable ==
               accounts[INVOKED_PROGRAM_DUP_INDEX].executable);
    break;
  }
  case RETURN_OK: {
    sol_log("return Ok");
    return SUCCESS;
  }
  case SET_RETURN_DATA: {
    sol_set_return_data((const uint8_t*)RETURN_DATA_VAL, sizeof(RETURN_DATA_VAL));
    sol_log("set return data");
    sol_assert(sol_get_return_data(NULL, 0, NULL) == sizeof(RETURN_DATA_VAL));
    return SUCCESS;
  }
  case RETURN_ERROR: {
    sol_log("return error");
    return 42;
  }
  case DERIVED_SIGNERS: {
    sol_log("verify derived signers");
    static const int INVOKED_PROGRAM_INDEX = 0;
    static const int DERIVED_KEY1_INDEX = 1;
    static const int DERIVED_KEY2_INDEX = 2;
    static const int DERIVED_KEY3_INDEX = 3;
    sol_assert(sol_deserialize(input, &params, 4));

    sol_assert(accounts[DERIVED_KEY1_INDEX].is_signer);
    sol_assert(!accounts[DERIVED_KEY2_INDEX].is_signer);
    sol_assert(!accounts[DERIVED_KEY2_INDEX].is_signer);

    uint8_t bump_seed2 = params.data[1];
    uint8_t bump_seed3 = params.data[2];

    SolAccountMeta arguments[] = {
        {accounts[DERIVED_KEY1_INDEX].key, true, false},
        {accounts[DERIVED_KEY2_INDEX].key, true, true},
        {accounts[DERIVED_KEY3_INDEX].key, false, true}};
    uint8_t data[] = {VERIFY_NESTED_SIGNERS};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SOL_ARRAY_SIZE(arguments),
                                        data, SOL_ARRAY_SIZE(data)};
    uint8_t seed1[] = {'L', 'i', 'l', '\''};
    uint8_t seed2[] = {'B', 'i', 't', 's'};
    const SolSignerSeed seeds1[] = {{seed1, SOL_ARRAY_SIZE(seed1)},
                                    {seed2, SOL_ARRAY_SIZE(seed2)},
                                    {&bump_seed2, 1}};
    const SolSignerSeed seeds2[] = {
        {(uint8_t *)accounts[DERIVED_KEY2_INDEX].key, SIZE_PUBKEY},
        {&bump_seed3, 1}};
    const SolSignerSeeds signers_seeds[] = {{seeds1, SOL_ARRAY_SIZE(seeds1)},
                                            {seeds2, SOL_ARRAY_SIZE(seeds2)}};

    sol_assert(SUCCESS == sol_invoke_signed(&instruction, accounts,
                                            params.ka_num, signers_seeds,
                                            SOL_ARRAY_SIZE(signers_seeds)));

    break;
  }

  case VERIFY_NESTED_SIGNERS: {
    sol_log("verify derived nested signers");
    static const int DERIVED_KEY1_INDEX = 0;
    static const int DERIVED_KEY2_INDEX = 1;
    static const int DERIVED_KEY3_INDEX = 2;
    sol_assert(sol_deserialize(input, &params, 3));

    sol_assert(!accounts[DERIVED_KEY1_INDEX].is_signer);
    sol_assert(accounts[DERIVED_KEY2_INDEX].is_signer);
    sol_assert(accounts[DERIVED_KEY2_INDEX].is_signer);

    break;
  }

  case VERIFY_WRITER: {
    sol_log("verify writable");
    static const int ARGUMENT_INDEX = 0;
    sol_assert(sol_deserialize(input, &params, 1));

    sol_assert(accounts[ARGUMENT_INDEX].is_writable);
    break;
  }

  case VERIFY_PRIVILEGE_ESCALATION: {
    sol_log("Verify privilege escalation");
    break;
  }

  case VERIFY_PRIVILEGE_DEESCALATION: {
    sol_log("verify privilege deescalation");
    static const int INVOKED_ARGUMENT_INDEX = 0;
    sol_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    sol_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    break;
  }
  case VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER: {
    sol_log("verify privilege deescalation escalation signer");
    static const int INVOKED_PROGRAM_INDEX = 0;
    static const int INVOKED_ARGUMENT_INDEX = 1;
    sol_assert(sol_deserialize(input, &params, 2));

    sol_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    sol_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    SolAccountMeta arguments[] = {
        {accounts[INVOKED_ARGUMENT_INDEX].key, true, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SOL_ARRAY_SIZE(arguments),
                                        data, SOL_ARRAY_SIZE(data)};
    sol_assert(SUCCESS ==
               sol_invoke(&instruction, accounts, SOL_ARRAY_SIZE(accounts)));
    break;
  }

  case VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE: {
    sol_log("verify privilege deescalation escalation writable");
    static const int INVOKED_PROGRAM_INDEX = 0;
    static const int INVOKED_ARGUMENT_INDEX = 1;
    sol_assert(sol_deserialize(input, &params, 2));

    sol_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    sol_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    SolAccountMeta arguments[] = {
        {accounts[INVOKED_ARGUMENT_INDEX].key, false, true}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SOL_ARRAY_SIZE(arguments),
                                        data, SOL_ARRAY_SIZE(data)};
    sol_assert(SUCCESS ==
               sol_invoke(&instruction, accounts, SOL_ARRAY_SIZE(accounts)));
    break;
  }

  case NESTED_INVOKE: {
    sol_log("invoke");

    static const int INVOKED_ARGUMENT_INDEX = 0;
    static const int ARGUMENT_INDEX = 1;
    static const int INVOKED_PROGRAM_INDEX = 2;

    if (!sol_deserialize(input, &params, 3)) {
      sol_assert(sol_deserialize(input, &params, 2));
    }

    sol_assert(sol_deserialize(input, &params, 2));

    sol_assert(accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    sol_assert(accounts[ARGUMENT_INDEX].is_signer);

    *accounts[INVOKED_ARGUMENT_INDEX].lamports -= 1;
    *accounts[ARGUMENT_INDEX].lamports += 1;

    uint8_t remaining_invokes = params.data[1];
    if (remaining_invokes > 1) {
      sol_log("Invoke again");
      SolAccountMeta arguments[] = {
          {accounts[INVOKED_ARGUMENT_INDEX].key, true, true},
          {accounts[ARGUMENT_INDEX].key, true, true},
          {accounts[INVOKED_PROGRAM_INDEX].key, false, false}};
      uint8_t data[] = {NESTED_INVOKE, remaining_invokes - 1};
      const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SOL_ARRAY_SIZE(arguments),
                                          data, SOL_ARRAY_SIZE(data)};
      sol_assert(SUCCESS == sol_invoke(&instruction, accounts, params.ka_num));
    } else {
      sol_log("Last invoked");
      for (int i = 0; i < accounts[INVOKED_ARGUMENT_INDEX].data_len; i++) {
        accounts[INVOKED_ARGUMENT_INDEX].data[i] = i;
      }
    }
    break;
  }

  case WRITE_ACCOUNT: {
    sol_log("write account");
    static const int INVOKED_ARGUMENT_INDEX = 0;
    sol_assert(sol_deserialize(input, &params, 1));

    for (int i = 0; i < params.data[1]; i++) {
      accounts[INVOKED_ARGUMENT_INDEX].data[i] = params.data[1];
    }
    break;
  }

  default:
    return ERROR_INVALID_INSTRUCTION_DATA;
  }
  return SUCCESS;
}
