/**
 * @brief Example C-based BPF program that tests cross-program invocations
 */
#include "../invoked/instruction.h"
#include <safecoin_sdk.h>

static const uint8_t TEST_SUCCESS = 1;
static const uint8_t TEST_PRIVILEGE_ESCALATION_SIGNER = 2;
static const uint8_t TEST_PRIVILEGE_ESCALATION_WRITABLE = 3;
static const uint8_t TEST_PPROGRAM_NOT_EXECUTABLE = 4;
static const uint8_t TEST_EMPTY_ACCOUNTS_SLICE = 5;
static const uint8_t TEST_CAP_SEEDS = 6;
static const uint8_t TEST_CAP_SIGNERS = 7;
static const uint8_t TEST_ALLOC_ACCESS_VIOLATION = 8;
static const uint8_t TEST_INSTRUCTION_DATA_TOO_LARGE = 9;
static const uint8_t TEST_INSTRUCTION_META_TOO_LARGE = 10;
static const uint8_t TEST_RETURN_ERROR = 11;
static const uint8_t TEST_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER = 12;
static const uint8_t TEST_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE = 13;
static const uint8_t TEST_WRITE_DEESCALATION = 14;

static const int MINT_INDEX = 0;
static const int ARGUMENT_INDEX = 1;
static const int INVOKED_PROGRAM_INDEX = 2;
static const int INVOKED_ARGUMENT_INDEX = 3;
static const int INVOKED_PROGRAM_DUP_INDEX = 4;
static const int ARGUMENT_DUP_INDEX = 5;
static const int DERIVED_KEY1_INDEX = 6;
static const int DERIVED_KEY2_INDEX = 7;
static const int DERIVED_KEY3_INDEX = 8;
static const int SYSTEM_PROGRAM_INDEX = 9;
static const int FROM_INDEX = 10;

extern uint64_t entrypoint(const uint8_t *input) {
  safe_log("Invoke C program");

  SafeAccountInfo accounts[11];
  SafeParameters params = (SafeParameters){.ka = accounts};

  if (!safe_deserialize(input, &params, SAFE_ARRAY_SIZE(accounts))) {
    return ERROR_INVALID_ARGUMENT;
  }

  uint8_t bump_seed1 = params.data[1];
  uint8_t bump_seed2 = params.data[2];
  uint8_t bump_seed3 = params.data[3];

  switch (params.data[0]) {
  case TEST_SUCCESS: {
    safe_log("Call system program create account");
    {
      uint64_t from_lamports = *accounts[FROM_INDEX].lamports;
      uint64_t to_lamports = *accounts[DERIVED_KEY1_INDEX].lamports;
      SafeAccountMeta arguments[] = {
          {accounts[FROM_INDEX].key, true, true},
          {accounts[DERIVED_KEY1_INDEX].key, true, true}};
      uint8_t data[4 + 8 + 8 + 32];
      *(uint64_t *)(data + 4) = 42;
      *(uint64_t *)(data + 4 + 8) = MAX_PERMITTED_DATA_INCREASE;
      safe_memcpy(data + 4 + 8 + 8, params.program_id, SIZE_PUBKEY);
      const SafeInstruction instruction = {accounts[SYSTEM_PROGRAM_INDEX].key,
                                          arguments, SAFE_ARRAY_SIZE(arguments),
                                          data, SAFE_ARRAY_SIZE(data)};
      uint8_t seed1[] = {'Y', 'o', 'u', ' ', 'p', 'a', 's', 's',
                         ' ', 'b', 'u', 't', 't', 'e', 'r'};
      const SafeSignerSeed seeds1[] = {{seed1, SAFE_ARRAY_SIZE(seed1)},
                                      {&bump_seed1, 1}};
      const SafeSignerSeeds signers_seeds[] = {{seeds1, SAFE_ARRAY_SIZE(seeds1)}};
      safe_assert(SUCCESS == safe_invoke_signed(&instruction, accounts,
                                              SAFE_ARRAY_SIZE(accounts),
                                              signers_seeds,
                                              SAFE_ARRAY_SIZE(signers_seeds)));
      safe_assert(*accounts[FROM_INDEX].lamports == from_lamports - 42);
      safe_assert(*accounts[DERIVED_KEY1_INDEX].lamports == to_lamports + 42);
      safe_assert(SafePubkey_same(accounts[DERIVED_KEY1_INDEX].owner,
                                params.program_id));
      safe_assert(accounts[DERIVED_KEY1_INDEX].data_len ==
                 MAX_PERMITTED_DATA_INCREASE);
      safe_assert(
          accounts[DERIVED_KEY1_INDEX].data[MAX_PERMITTED_DATA_INCREASE - 1] ==
          0);
      accounts[DERIVED_KEY1_INDEX].data[MAX_PERMITTED_DATA_INCREASE - 1] = 0x0f;
      safe_assert(
          accounts[DERIVED_KEY1_INDEX].data[MAX_PERMITTED_DATA_INCREASE - 1] ==
          0x0f);
      for (uint8_t i = 0; i < 20; i++) {
        accounts[DERIVED_KEY1_INDEX].data[i] = i;
      }
    }

    safe_log("Call system program transfer");
    {
      uint64_t from_lamports = *accounts[FROM_INDEX].lamports;
      uint64_t to_lamports = *accounts[DERIVED_KEY1_INDEX].lamports;
      SafeAccountMeta arguments[] = {
          {accounts[FROM_INDEX].key, true, true},
          {accounts[DERIVED_KEY1_INDEX].key, true, false}};
      uint8_t data[] = {2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0};
      const SafeInstruction instruction = {accounts[SYSTEM_PROGRAM_INDEX].key,
                                          arguments, SAFE_ARRAY_SIZE(arguments),
                                          data, SAFE_ARRAY_SIZE(data)};
      safe_assert(SUCCESS ==
                 safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));
      safe_assert(*accounts[FROM_INDEX].lamports == from_lamports - 1);
      safe_assert(*accounts[DERIVED_KEY1_INDEX].lamports == to_lamports + 1);
    }

    safe_log("Test data translation");
    {
      for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
        accounts[ARGUMENT_INDEX].data[i] = i;
      }

      SafeAccountMeta arguments[] = {
          {accounts[ARGUMENT_INDEX].key, true, true},
          {accounts[INVOKED_ARGUMENT_INDEX].key, true, true},
          {accounts[INVOKED_PROGRAM_INDEX].key, false, false},
          {accounts[INVOKED_PROGRAM_DUP_INDEX].key, false, false}};
      uint8_t data[] = {VERIFY_TRANSLATIONS, 1, 2, 3, 4, 5};
      const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAFE_ARRAY_SIZE(arguments),
                                          data, SAFE_ARRAY_SIZE(data)};

      safe_assert(SUCCESS ==
                 safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));
    }

    safe_log("Test no instruction data");
    {
      SafeAccountMeta arguments[] = {{accounts[ARGUMENT_INDEX].key, true, true}};
      uint8_t data[] = {};
      const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAFE_ARRAY_SIZE(arguments),
                                          data, SAFE_ARRAY_SIZE(data)};

      safe_assert(SUCCESS ==
                 safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));
    }

    safe_log("Test create_program_address");
    {
      uint8_t seed1[] = {'Y', 'o', 'u', ' ', 'p', 'a', 's', 's',
                         ' ', 'b', 'u', 't', 't', 'e', 'r'};
      const SafeSignerSeed seeds1[] = {{seed1, SAFE_ARRAY_SIZE(seed1)},
                                      {&bump_seed1, 1}};
      SafePubkey address;
      safe_assert(SUCCESS ==
                 safe_create_program_address(seeds1, SAFE_ARRAY_SIZE(seeds1),
                                            params.program_id, &address));
      safe_assert(SafePubkey_same(&address, accounts[DERIVED_KEY1_INDEX].key));
    }

    safe_log("Test try_find_program_address");
    {
      uint8_t seed[] = {'Y', 'o', 'u', ' ', 'p', 'a', 's', 's',
                        ' ', 'b', 'u', 't', 't', 'e', 'r'};
      const SafeSignerSeed seeds[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
      SafePubkey address;
      uint8_t bump_seed;
      safe_assert(SUCCESS == safe_try_find_program_address(
                                seeds, SAFE_ARRAY_SIZE(seeds), params.program_id,
                                &address, &bump_seed));
      safe_assert(SafePubkey_same(&address, accounts[DERIVED_KEY1_INDEX].key));
      safe_assert(bump_seed == bump_seed1);
    }

    safe_log("Test derived signers");
    {
      safe_assert(!accounts[DERIVED_KEY1_INDEX].is_signer);
      safe_assert(!accounts[DERIVED_KEY2_INDEX].is_signer);
      safe_assert(!accounts[DERIVED_KEY3_INDEX].is_signer);

      SafeAccountMeta arguments[] = {
          {accounts[INVOKED_PROGRAM_INDEX].key, false, false},
          {accounts[DERIVED_KEY1_INDEX].key, true, true},
          {accounts[DERIVED_KEY2_INDEX].key, true, false},
          {accounts[DERIVED_KEY3_INDEX].key, false, false}};
      uint8_t data[] = {DERIVED_SIGNERS, bump_seed2, bump_seed3};
      const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAFE_ARRAY_SIZE(arguments),
                                          data, SAFE_ARRAY_SIZE(data)};
      uint8_t seed1[] = {'Y', 'o', 'u', ' ', 'p', 'a', 's', 's',
                         ' ', 'b', 'u', 't', 't', 'e', 'r'};
      const SafeSignerSeed seeds1[] = {{seed1, SAFE_ARRAY_SIZE(seed1)},
                                      {&bump_seed1, 1}};
      const SafeSignerSeeds signers_seeds[] = {{seeds1, SAFE_ARRAY_SIZE(seeds1)}};
      safe_assert(SUCCESS == safe_invoke_signed(&instruction, accounts,
                                              SAFE_ARRAY_SIZE(accounts),
                                              signers_seeds,
                                              SAFE_ARRAY_SIZE(signers_seeds)));
    }

    safe_log("Test readonly with writable account");
    {
      SafeAccountMeta arguments[] = {
          {accounts[INVOKED_ARGUMENT_INDEX].key, true, false}};
      uint8_t data[] = {VERIFY_WRITER};
      const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAFE_ARRAY_SIZE(arguments),
                                          data, SAFE_ARRAY_SIZE(data)};

      safe_assert(SUCCESS ==
                 safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));
    }

    safe_log("Test invoke");
    {
      safe_assert(accounts[ARGUMENT_INDEX].is_signer);

      *accounts[ARGUMENT_INDEX].lamports -= 5;
      *accounts[INVOKED_ARGUMENT_INDEX].lamports += 5;

      SafeAccountMeta arguments[] = {
          {accounts[INVOKED_ARGUMENT_INDEX].key, true, true},
          {accounts[ARGUMENT_INDEX].key, true, true},
          {accounts[INVOKED_PROGRAM_DUP_INDEX].key, false, false}};
      uint8_t data[] = {NESTED_INVOKE};
      const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAFE_ARRAY_SIZE(arguments),
                                          data, SAFE_ARRAY_SIZE(data)};

      safe_log("First invoke");
      safe_assert(SUCCESS ==
                 safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));
      safe_log("2nd invoke from first program");
      safe_assert(SUCCESS ==
                 safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));

      safe_assert(*accounts[ARGUMENT_INDEX].lamports == 42 - 5 + 1 + 1 + 1 + 1);
      safe_assert(*accounts[INVOKED_ARGUMENT_INDEX].lamports ==
                 10 + 5 - 1 - 1 - 1 - 1);
    }

    safe_log("Test privilege deescalation");
    {
      safe_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
      safe_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
      SafeAccountMeta arguments[] = {
          {accounts[INVOKED_ARGUMENT_INDEX].key, false, false}};
      uint8_t data[] = {VERIFY_PRIVILEGE_DEESCALATION};
      const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAFE_ARRAY_SIZE(arguments),
                                          data, SAFE_ARRAY_SIZE(data)};
      safe_assert(SUCCESS ==
                 safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));
    }

    safe_log("Verify data values are retained and updated");
    for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
      safe_assert(accounts[ARGUMENT_INDEX].data[i] == i);
    }
    for (int i = 0; i < accounts[INVOKED_ARGUMENT_INDEX].data_len; i++) {
      safe_assert(accounts[INVOKED_ARGUMENT_INDEX].data[i] == i);
    }

    safe_log("Verify data write before ro cpi call");
    {
      for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
        accounts[ARGUMENT_INDEX].data[i] = 0;
      }

      SafeAccountMeta arguments[] = {
          {accounts[ARGUMENT_INDEX].key, false, false}};
      uint8_t data[] = {VERIFY_PRIVILEGE_DEESCALATION};
      const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAFE_ARRAY_SIZE(arguments),
                                          data, SAFE_ARRAY_SIZE(data)};
      safe_assert(SUCCESS ==
                 safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));

      for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
        safe_assert(accounts[ARGUMENT_INDEX].data[i] == 0);
      }
    }
    break;
  }
  case TEST_PRIVILEGE_ESCALATION_SIGNER: {
    safe_log("Test privilege escalation signer");
    SafeAccountMeta arguments[] = {
        {accounts[DERIVED_KEY3_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    safe_assert(SUCCESS ==
               safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));

    // Signer privilege escalation will always fail the whole transaction
    instruction.accounts[0].is_signer = true;
    safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts));
    break;
  }
  case TEST_PRIVILEGE_ESCALATION_WRITABLE: {
    safe_log("Test privilege escalation writable");
    SafeAccountMeta arguments[] = {
        {accounts[DERIVED_KEY3_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    safe_assert(SUCCESS ==
               safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));

    // Writable privilege escalation will always fail the whole transaction
    instruction.accounts[0].is_writable = true;
    safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts));
    break;
  }
  case TEST_PPROGRAM_NOT_EXECUTABLE: {
    safe_log("Test program not executable");
    SafeAccountMeta arguments[] = {
        {accounts[DERIVED_KEY3_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SafeInstruction instruction = {accounts[ARGUMENT_INDEX].key, arguments,
                                        SAFE_ARRAY_SIZE(arguments), data,
                                        SAFE_ARRAY_SIZE(data)};
    return safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts));
  }
  case TEST_EMPTY_ACCOUNTS_SLICE: {
    safe_log("Empty accounts slice");

    SafeAccountMeta arguments[] = {
        {accounts[INVOKED_ARGUMENT_INDEX].key, false, false}};
    uint8_t data[] = {};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};

    safe_assert(SUCCESS == safe_invoke(&instruction, 0, 0));
    break;
  }
  case TEST_CAP_SEEDS: {
    safe_log("Test cap seeds");
    SafeAccountMeta arguments[] = {};
    uint8_t data[] = {};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    uint8_t seed[] = {"seed"};
    const SafeSignerSeed seeds[] = {
        {seed, SAFE_ARRAY_SIZE(seed)}, {seed, SAFE_ARRAY_SIZE(seed)},
        {seed, SAFE_ARRAY_SIZE(seed)}, {seed, SAFE_ARRAY_SIZE(seed)},
        {seed, SAFE_ARRAY_SIZE(seed)}, {seed, SAFE_ARRAY_SIZE(seed)},
        {seed, SAFE_ARRAY_SIZE(seed)}, {seed, SAFE_ARRAY_SIZE(seed)},
        {seed, SAFE_ARRAY_SIZE(seed)}, {seed, SAFE_ARRAY_SIZE(seed)},
        {seed, SAFE_ARRAY_SIZE(seed)}, {seed, SAFE_ARRAY_SIZE(seed)},
        {seed, SAFE_ARRAY_SIZE(seed)}, {seed, SAFE_ARRAY_SIZE(seed)},
        {seed, SAFE_ARRAY_SIZE(seed)}, {seed, SAFE_ARRAY_SIZE(seed)},
        {seed, SAFE_ARRAY_SIZE(seed)},
    };
    const SafeSignerSeeds signers_seeds[] = {{seeds, SAFE_ARRAY_SIZE(seeds)}};
    safe_assert(SUCCESS == safe_invoke_signed(
                              &instruction, accounts, SAFE_ARRAY_SIZE(accounts),
                              signers_seeds, SAFE_ARRAY_SIZE(signers_seeds)));
    break;
  }
  case TEST_CAP_SIGNERS: {
    safe_log("Test cap signers");
    SafeAccountMeta arguments[] = {};
    uint8_t data[] = {};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    uint8_t seed[] = {"seed"};
    const SafeSignerSeed seed1[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed2[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed3[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed4[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed5[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed6[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed7[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed8[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed9[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed10[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed11[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed12[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed13[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed14[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed15[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed16[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeed seed17[] = {{seed, SAFE_ARRAY_SIZE(seed)}};
    const SafeSignerSeeds signers_seeds[] = {
        {seed1, SAFE_ARRAY_SIZE(seed1)},   {seed2, SAFE_ARRAY_SIZE(seed2)},
        {seed3, SAFE_ARRAY_SIZE(seed3)},   {seed4, SAFE_ARRAY_SIZE(seed4)},
        {seed5, SAFE_ARRAY_SIZE(seed5)},   {seed6, SAFE_ARRAY_SIZE(seed6)},
        {seed7, SAFE_ARRAY_SIZE(seed7)},   {seed8, SAFE_ARRAY_SIZE(seed8)},
        {seed9, SAFE_ARRAY_SIZE(seed9)},   {seed10, SAFE_ARRAY_SIZE(seed10)},
        {seed11, SAFE_ARRAY_SIZE(seed11)}, {seed12, SAFE_ARRAY_SIZE(seed12)},
        {seed13, SAFE_ARRAY_SIZE(seed13)}, {seed14, SAFE_ARRAY_SIZE(seed14)},
        {seed15, SAFE_ARRAY_SIZE(seed15)}, {seed16, SAFE_ARRAY_SIZE(seed16)},
        {seed17, SAFE_ARRAY_SIZE(seed17)}};
    safe_assert(SUCCESS == safe_invoke_signed(
                              &instruction, accounts, SAFE_ARRAY_SIZE(accounts),
                              signers_seeds, SAFE_ARRAY_SIZE(signers_seeds)));
    break;
  }
  case TEST_ALLOC_ACCESS_VIOLATION: {
    safe_log("Test resize violation");
    SafeAccountMeta arguments[] = {
        {accounts[FROM_INDEX].key, true, true},
        {accounts[DERIVED_KEY1_INDEX].key, true, true}};
    uint8_t data[4 + 8 + 8 + 32];
    *(uint64_t *)(data + 4) = 42;
    *(uint64_t *)(data + 4 + 8) = MAX_PERMITTED_DATA_INCREASE;
    safe_memcpy(data + 4 + 8 + 8, params.program_id, SIZE_PUBKEY);
    const SafeInstruction instruction = {accounts[SYSTEM_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    uint8_t seed1[] = {'Y', 'o', 'u', ' ', 'p', 'a', 's', 's',
                       ' ', 'b', 'u', 't', 't', 'e', 'r'};
    const SafeSignerSeed seeds1[] = {{seed1, SAFE_ARRAY_SIZE(seed1)},
                                    {&bump_seed1, 1}};
    const SafeSignerSeeds signers_seeds[] = {{seeds1, SAFE_ARRAY_SIZE(seeds1)}};

    SafeAccountInfo derived_account = {
        .key = accounts[DERIVED_KEY1_INDEX].key,
        .lamports = accounts[DERIVED_KEY1_INDEX].lamports,
        .data_len = accounts[DERIVED_KEY1_INDEX].data_len,
        // Point to top edge of heap, attempt to allocate into unprivileged
        // memory
        .data = (uint8_t *)0x300007ff8,
        .owner = accounts[DERIVED_KEY1_INDEX].owner,
        .rent_epoch = accounts[DERIVED_KEY1_INDEX].rent_epoch,
        .is_signer = accounts[DERIVED_KEY1_INDEX].is_signer,
        .is_writable = accounts[DERIVED_KEY1_INDEX].is_writable,
        .executable = accounts[DERIVED_KEY1_INDEX].executable,
    };
    const SafeAccountInfo invoke_accounts[] = {
        accounts[FROM_INDEX], accounts[SYSTEM_PROGRAM_INDEX], derived_account};
    safe_assert(SUCCESS ==
               safe_invoke_signed(&instruction,
                                 (const SafeAccountInfo *)invoke_accounts, 3,
                                 signers_seeds, SAFE_ARRAY_SIZE(signers_seeds)));
    break;
  }
  case TEST_INSTRUCTION_DATA_TOO_LARGE: {
    safe_log("Test instruction data too large");
    SafeAccountMeta arguments[] = {};
    uint8_t *data = safe_calloc(1500, 1);
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, 1500};
    const SafeSignerSeeds signers_seeds[] = {};
    safe_assert(SUCCESS == safe_invoke_signed(
                              &instruction, accounts, SAFE_ARRAY_SIZE(accounts),
                              signers_seeds, SAFE_ARRAY_SIZE(signers_seeds)));

    break;
  }
  case TEST_INSTRUCTION_META_TOO_LARGE: {
    safe_log("Test instruction meta too large");
    SafeAccountMeta *arguments = safe_calloc(40, sizeof(SafeAccountMeta));
    safe_log_64(0, 0, 0, 0, (uint64_t)arguments);
    safe_assert(0 != arguments);
    uint8_t data[] = {};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, 40, data,
                                        SAFE_ARRAY_SIZE(data)};
    const SafeSignerSeeds signers_seeds[] = {};
    safe_assert(SUCCESS == safe_invoke_signed(
                              &instruction, accounts, SAFE_ARRAY_SIZE(accounts),
                              signers_seeds, SAFE_ARRAY_SIZE(signers_seeds)));

    break;
  }
  case TEST_RETURN_ERROR: {
    safe_log("Test return error");
    SafeAccountMeta arguments[] = {{accounts[ARGUMENT_INDEX].key, false, true}};
    uint8_t data[] = {RETURN_ERROR};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};

    safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts));
    break;
  }

  case TEST_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER: {
    safe_log("Test privilege deescalation escalation signer");
    safe_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    safe_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    SafeAccountMeta arguments[] = {
        {accounts[INVOKED_PROGRAM_INDEX].key, false, false},
        {accounts[INVOKED_ARGUMENT_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    safe_assert(SUCCESS ==
               safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));
    break;
  }
  case TEST_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE: {
    safe_log("Test privilege deescalation escalation writable");
    safe_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    safe_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    SafeAccountMeta arguments[] = {
        {accounts[INVOKED_PROGRAM_INDEX].key, false, false},
        {accounts[INVOKED_ARGUMENT_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    safe_assert(SUCCESS ==
               safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts)));
    break;
  }

  case TEST_WRITE_DEESCALATION: {
    safe_log("Test writable deescalation");

    SafeAccountMeta arguments[] = {
        {accounts[INVOKED_ARGUMENT_INDEX].key, false, false}};
    uint8_t data[] = {WRITE_ACCOUNT, 10};
    const SafeInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAFE_ARRAY_SIZE(arguments),
                                        data, SAFE_ARRAY_SIZE(data)};
    safe_invoke(&instruction, accounts, SAFE_ARRAY_SIZE(accounts));
    break;
  }
  default:
    sol_panic();
  }

  return SUCCESS;
}
