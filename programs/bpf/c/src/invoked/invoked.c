/**
 * @brief Example C-based BPF program that tests cross-program invocations
 */
#include "instruction.h"
#include <safecoin_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  safe_log("Invoked C program");

  SafeAccountInfo accounts[4];
  SafeParameters params = (SafeParameters){.ka = accounts};

  if (!safe_deserialize(input, &params, 0)) {
    return ERROR_INVALID_ARGUMENT;
  }

  if (params.data_len == 0) {
    return SUCCESS;
  }

  switch (params.data[0]) {
  case VERIFY_TRANSLATIONS: {
    safe_log("verify data translations");

    static const int ARGUMENT_INDEX = 0;
    static const int INVOKED_ARGUMENT_INDEX = 1;
    static const int INVOKED_PROGRAM_INDEX = 2;
    static const int INVOKED_PROGRAM_DUP_INDEX = 3;
    safe_assert(safe_deserialize(input, &params, 4));

    SafePubkey bpf_loader_id =
        (SafePubkey){.x = {2,  168, 246, 145, 78,  136, 161, 110, 57,  90, 225,
                          40, 148, 143, 250, 105, 86,  147, 55,  104, 24, 221,
                          71, 67,  82,  33,  243, 198, 0,   0,   0,   0}};

    SafePubkey bpf_loader_deprecated_id =
        (SafePubkey){.x = {2,   168, 246, 145, 78,  136, 161, 107, 189, 35,  149,
                          133, 95,  100, 4,   217, 180, 244, 86,  183, 130, 27,
                          176, 20,  87,  73,  66,  140, 0,   0,   0,   0}};

    for (int i = 0; i < params.data_len; i++) {
      safe_assert(params.data[i] == i);
    }
    safe_assert(params.ka_num == 4);

    safe_assert(*accounts[ARGUMENT_INDEX].lamports == 42);
    safe_assert(accounts[ARGUMENT_INDEX].data_len == 100);
    safe_assert(accounts[ARGUMENT_INDEX].is_signer);
    safe_assert(accounts[ARGUMENT_INDEX].is_writable);
    safe_assert(accounts[ARGUMENT_INDEX].rent_epoch == 0);
    safe_assert(!accounts[ARGUMENT_INDEX].executable);
    for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
      safe_assert(accounts[ARGUMENT_INDEX].data[i] == i);
    }

    safe_assert(SafePubkey_same(accounts[INVOKED_ARGUMENT_INDEX].owner,
                              accounts[INVOKED_PROGRAM_INDEX].key));
    safe_assert(*accounts[INVOKED_ARGUMENT_INDEX].lamports == 10);
    safe_assert(accounts[INVOKED_ARGUMENT_INDEX].data_len == 10);
    safe_assert(accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    safe_assert(accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    safe_assert(accounts[INVOKED_ARGUMENT_INDEX].rent_epoch == 0);
    safe_assert(!accounts[INVOKED_ARGUMENT_INDEX].executable);

    safe_assert(
        SafePubkey_same(accounts[INVOKED_PROGRAM_INDEX].key, params.program_id))
        safe_assert(SafePubkey_same(accounts[INVOKED_PROGRAM_INDEX].owner,
                                  &bpf_loader_id));
    safe_assert(!accounts[INVOKED_PROGRAM_INDEX].is_signer);
    safe_assert(!accounts[INVOKED_PROGRAM_INDEX].is_writable);
    safe_assert(accounts[INVOKED_PROGRAM_INDEX].rent_epoch == 0);
    safe_assert(accounts[INVOKED_PROGRAM_INDEX].executable);

    safe_assert(SafePubkey_same(accounts[INVOKED_PROGRAM_INDEX].key,
                              accounts[INVOKED_PROGRAM_DUP_INDEX].key));
    safe_assert(SafePubkey_same(accounts[INVOKED_PROGRAM_INDEX].owner,
                              accounts[INVOKED_PROGRAM_DUP_INDEX].owner));
    safe_assert(*accounts[INVOKED_PROGRAM_INDEX].lamports ==
               *accounts[INVOKED_PROGRAM_DUP_INDEX].lamports);
    safe_assert(accounts[INVOKED_PROGRAM_INDEX].is_signer ==
               accounts[INVOKED_PROGRAM_DUP_INDEX].is_signer);
    safe_assert(accounts[INVOKED_PROGRAM_INDEX].is_writable ==
               accounts[INVOKED_PROGRAM_DUP_INDEX].is_writable);
    safe_assert(accounts[INVOKED_PROGRAM_INDEX].rent_epoch ==
               accounts[INVOKED_PROGRAM_DUP_INDEX].rent_epoch);
    safe_assert(accounts[INVOKED_PROGRAM_INDEX].executable ==
               accounts[INVOKED_PROGRAM_DUP_INDEX].executable);
    break;
  }
  case RETURN_OK: {
    safe_log("return Ok");
    return SUCCESS;
  }
  case RETURN_ERROR: {
    safe_log("return error");
    return 42;
  }
  case DERIVED_SIGNERS: {
    safe_log("verify derived signers");
    static const int INVOKED_PROGRAM_INDEX = 0;
    static const int DERIVED_KEY1_INDEX = 1;
    static const int DERIVED_KEY2_INDEX = 2;
    static const int DERIVED_KEY3_INDEX = 3;
    safe_assert(safe_deserialize(input, &params, 4));

    safe_assert(accounts[DERIVED_KEY1_INDEX].is_signer);
    safe_assert(!accounts[DERIVED_KEY2_INDEX].is_signer);
    safe_assert(!accounts[DERIVED_KEY2_INDEX].is_signer);

    uint8_t bump_seed2 = params.data[1];
    uint8_t bump_seed3 = params.data[2];

    SafeAccountMeta arguments[] = {
        {accounts[DERIVED_KEY1_INDEX].key, true, false},
        {accounts[DERIVED_KEY2_INDEX].key, true, true},
        {accounts[DERIVED_KEY3_INDEX].key, false, true}};
    uint8_t data[] = {VERIFY_NESTED_SIGNERS};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    uint8_t seed1[] = {'L', 'i', 'l', '\''};
    uint8_t seed2[] = {'B', 'i', 't', 's'};
    const SafeSignerSeed seeds1[] = {{seed1, SAFE_ARRAY_SIZE(seed1)},
                                    {seed2, SAFE_ARRAY_SIZE(seed2)},
                                    {&bump_seed2, 1}};
    const SafeSignerSeed seeds2[] = {
        {(uint8_t *)accounts[DERIVED_KEY2_INDEX].key, SIZE_PUBKEY},
        {&bump_seed3, 1}};
    const SafeSignerSeeds signers_seeds[] = {{seeds1, SAFE_ARRAY_SIZE(seeds1)},
                                            {seeds2, SAFE_ARRAY_SIZE(seeds2)}};

    safe_assert(SUCCESS == safe_invoke_signed(&instruction, accounts,
                                            params.ka_num, signers_seeds,
                                            SAFE_ARRAY_SIZE(signers_seeds)));

    break;
  }

  case VERIFY_NESTED_SIGNERS: {
    safe_log("verify derived nested signers");
    static const int DERIVED_KEY1_INDEX = 0;
    static const int DERIVED_KEY2_INDEX = 1;
    static const int DERIVED_KEY3_INDEX = 2;
    safe_assert(safe_deserialize(input, &params, 3));

    safe_assert(!accounts[DERIVED_KEY1_INDEX].is_signer);
    safe_assert(accounts[DERIVED_KEY2_INDEX].is_signer);
    safe_assert(accounts[DERIVED_KEY2_INDEX].is_signer);

    break;
  }

  case VERIFY_WRITER: {
    safe_log("verify writable");
    static const int ARGUMENT_INDEX = 0;
    safe_assert(safe_deserialize(input, &params, 1));

    safe_assert(accounts[ARGUMENT_INDEX].is_writable);
    break;
  }

  case VERIFY_PRIVILEGE_ESCALATION: {
    safe_log("Should never get here!");
    break;
  }

  case VERIFY_PRIVILEGE_DEESCALATION: {
    safe_log("verify privilege deescalation");
    static const int INVOKED_ARGUMENT_INDEX = 0;
    safe_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    safe_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    break;
  }
  case VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER: {
    safe_log("verify privilege deescalation escalation signer");
    static const int INVOKED_PROGRAM_INDEX = 0;
    static const int INVOKED_ARGUMENT_INDEX = 1;
    safe_assert(safe_deserialize(input, &params, 2));

    safe_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    safe_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    SafeAccountMeta arguments[] = {
        {accounts[INVOKED_ARGUMENT_INDEX].key, true, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    safe_assert(SUCCESS ==
               safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));
    break;
  }

  case VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE: {
    safe_log("verify privilege deescalation escalation writable");
    static const int INVOKED_PROGRAM_INDEX = 0;
    static const int INVOKED_ARGUMENT_INDEX = 1;
    safe_assert(safe_deserialize(input, &params, 2));

    safe_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    safe_assert(false == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    SafeAccountMeta arguments[] = {
        {accounts[INVOKED_ARGUMENT_INDEX].key, false, true}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    safe_assert(SUCCESS ==
               safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));
    break;
  }

  case NESTED_INVOKE: {
    safe_log("invoke");

    static const int INVOKED_ARGUMENT_INDEX = 0;
    static const int ARGUMENT_INDEX = 1;
    static const int INVOKED_PROGRAM_INDEX = 2;

    if (!safe_deserialize(input, &params, 3)) {
      safe_assert(safe_deserialize(input, &params, 2));
    }

    safe_assert(safe_deserialize(input, &params, 2));

    safe_assert(accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    safe_assert(accounts[ARGUMENT_INDEX].is_signer);

    *accounts[INVOKED_ARGUMENT_INDEX].lamports -= 1;
    *accounts[ARGUMENT_INDEX].lamports += 1;

    if (params.ka_num == 3) {
      SafeAccountMeta arguments[] = {
          {accounts[INVOKED_ARGUMENT_INDEX].key, true, true},
          {accounts[ARGUMENT_INDEX].key, true, true}};
      uint8_t data[] = {NESTED_INVOKE};
      const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAFE_ARRAY_SIZE(arguments),
                                          data, SAFE_ARRAY_SIZE(data)};

      safe_log("Invoke again");
      safe_assert(SUCCESS == safe_invoke(&instruction, accounts, params.ka_num));
    } else {
      safe_log("Last invoked");
      for (int i = 0; i < accounts[INVOKED_ARGUMENT_INDEX].data_len; i++) {
        accounts[INVOKED_ARGUMENT_INDEX].data[i] = i;
      }
    }
    break;
  }

  case WRITE_ACCOUNT: {
    safe_log("write account");
    static const int INVOKED_ARGUMENT_INDEX = 0;
    safe_assert(safe_deserialize(input, &params, 1));

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
