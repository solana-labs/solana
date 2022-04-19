/**
 * @brief Example C-based BPF program that tests cross-program invocations
 */
#include "../invoked/instruction.h"
#include <sand/entrypoint.h>
#include <sand/cpi.h>
#include <sand/pubkey.h>
#include <sand/log.h>
#include <sand/assert.h>
#include <sand/deserialize.h>
#include <sand/return_data.h>

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
static const uint8_t TEST_WRITABLE_DEESCALATION_WRITABLE = 14;
static const uint8_t TEST_NESTED_INVOKE_TOO_DEEP = 15;
static const uint8_t TEST_EXECUTABLE_LAMPORTS = 16;
static const uint8_t TEST_CALL_PRECOMPILE = 17;
static const uint8_t ADD_LAMPORTS = 18;
static const uint8_t TEST_RETURN_DATA_TOO_LARGE = 19;
static const uint8_t TEST_DUPLICATE_PRIVILEGE_ESCALATION_SIGNER = 20;
static const uint8_t TEST_DUPLICATE_PRIVILEGE_ESCALATION_WRITABLE = 21;

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
static const int ED25519_PROGRAM_INDEX = 11;
static const int INVOKE_PROGRAM_INDEX = 12;

uint64_t do_nested_invokes(uint64_t num_nested_invokes,
                           SolAccountInfo *accounts, uint64_t num_accounts) {
  sand_assert(accounts[ARGUMENT_INDEX].is_signer);

  *accounts[ARGUMENT_INDEX].lamports -= 5;
  *accounts[INVOKED_ARGUMENT_INDEX].lamports += 5;

  SolAccountMeta arguments[] = {
      {accounts[INVOKED_ARGUMENT_INDEX].key, true, true},
      {accounts[ARGUMENT_INDEX].key, true, true},
      {accounts[INVOKED_PROGRAM_INDEX].key, false, false}};
  uint8_t data[] = {NESTED_INVOKE, num_nested_invokes};
  const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                      arguments, SAND_ARRAY_SIZE(arguments),
                                      data, SAND_ARRAY_SIZE(data)};

  sand_log("First invoke");
  sand_assert(SUCCESS == sand_invoke(&instruction, accounts, num_accounts));
  sand_log("2nd invoke from first program");
  sand_assert(SUCCESS == sand_invoke(&instruction, accounts, num_accounts));

  sand_assert(*accounts[ARGUMENT_INDEX].lamports ==
             42 - 5 + (2 * num_nested_invokes));
  sand_assert(*accounts[INVOKED_ARGUMENT_INDEX].lamports ==
             10 + 5 - (2 * num_nested_invokes));

  return SUCCESS;
}

extern uint64_t entrypoint(const uint8_t *input) {
  sand_log("Invoke C program");

  SolAccountInfo accounts[13];
  SolParameters params = (SolParameters){.ka = accounts};

  if (!sand_deserialize(input, &params, SAND_ARRAY_SIZE(accounts))) {
    return ERROR_INVALID_ARGUMENT;
  }

  uint8_t bump_seed1 = params.data[1];
  uint8_t bump_seed2 = params.data[2];
  uint8_t bump_seed3 = params.data[3];

  switch (params.data[0]) {
  case TEST_SUCCESS: {
    sand_log("Call system program create account");
    {
      uint64_t from_lamports = *accounts[FROM_INDEX].lamports;
      uint64_t to_lamports = *accounts[DERIVED_KEY1_INDEX].lamports;
      SolAccountMeta arguments[] = {
          {accounts[FROM_INDEX].key, true, true},
          {accounts[DERIVED_KEY1_INDEX].key, true, true}};
      uint8_t data[4 + 8 + 8 + 32];
      *(uint64_t *)(data + 4) = 42;
      *(uint64_t *)(data + 4 + 8) = MAX_PERMITTED_DATA_INCREASE;
      sand_memcpy(data + 4 + 8 + 8, params.program_id, SIZE_PUBKEY);
      const SolInstruction instruction = {accounts[SYSTEM_PROGRAM_INDEX].key,
                                          arguments, SAND_ARRAY_SIZE(arguments),
                                          data, SAND_ARRAY_SIZE(data)};
      uint8_t seed1[] = {'Y', 'o', 'u', ' ', 'p', 'a', 's', 's',
                         ' ', 'b', 'u', 't', 't', 'e', 'r'};
      const SolSignerSeed seeds1[] = {{seed1, SAND_ARRAY_SIZE(seed1)},
                                      {&bump_seed1, 1}};
      const SolSignerSeeds signers_seeds[] = {{seeds1, SAND_ARRAY_SIZE(seeds1)}};
      sand_assert(SUCCESS == sand_invoke_signed(&instruction, accounts,
                                              SAND_ARRAY_SIZE(accounts),
                                              signers_seeds,
                                              SAND_ARRAY_SIZE(signers_seeds)));
      sand_assert(*accounts[FROM_INDEX].lamports == from_lamports - 42);
      sand_assert(*accounts[DERIVED_KEY1_INDEX].lamports == to_lamports + 42);
      sand_assert(SolPubkey_same(accounts[DERIVED_KEY1_INDEX].owner,
                                params.program_id));
      sand_assert(accounts[DERIVED_KEY1_INDEX].data_len ==
                 MAX_PERMITTED_DATA_INCREASE);
      sand_assert(
          accounts[DERIVED_KEY1_INDEX].data[MAX_PERMITTED_DATA_INCREASE - 1] ==
          0);
      accounts[DERIVED_KEY1_INDEX].data[MAX_PERMITTED_DATA_INCREASE - 1] = 0x0f;
      sand_assert(
          accounts[DERIVED_KEY1_INDEX].data[MAX_PERMITTED_DATA_INCREASE - 1] ==
          0x0f);
      for (uint8_t i = 0; i < 20; i++) {
        accounts[DERIVED_KEY1_INDEX].data[i] = i;
      }
    }

    sand_log("Call system program transfer");
    {
      uint64_t from_lamports = *accounts[FROM_INDEX].lamports;
      uint64_t to_lamports = *accounts[DERIVED_KEY1_INDEX].lamports;
      SolAccountMeta arguments[] = {
          {accounts[FROM_INDEX].key, true, true},
          {accounts[DERIVED_KEY1_INDEX].key, true, false}};
      uint8_t data[] = {2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0};
      const SolInstruction instruction = {accounts[SYSTEM_PROGRAM_INDEX].key,
                                          arguments, SAND_ARRAY_SIZE(arguments),
                                          data, SAND_ARRAY_SIZE(data)};
      sand_assert(SUCCESS ==
                 sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));
      sand_assert(*accounts[FROM_INDEX].lamports == from_lamports - 1);
      sand_assert(*accounts[DERIVED_KEY1_INDEX].lamports == to_lamports + 1);
    }

    sand_log("Test data translation");
    {
      for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
        accounts[ARGUMENT_INDEX].data[i] = i;
      }

      SolAccountMeta arguments[] = {
          {accounts[ARGUMENT_INDEX].key, true, true},
          {accounts[INVOKED_ARGUMENT_INDEX].key, true, true},
          {accounts[INVOKED_PROGRAM_INDEX].key, false, false},
          {accounts[INVOKED_PROGRAM_DUP_INDEX].key, false, false}};
      uint8_t data[] = {VERIFY_TRANSLATIONS, 1, 2, 3, 4, 5};
      const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAND_ARRAY_SIZE(arguments),
                                          data, SAND_ARRAY_SIZE(data)};

      sand_assert(SUCCESS ==
                 sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));
    }

    sand_log("Test no instruction data");
    {
      SolAccountMeta arguments[] = {{accounts[ARGUMENT_INDEX].key, true, true}};
      uint8_t data[] = {};
      const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAND_ARRAY_SIZE(arguments),
                                          data, SAND_ARRAY_SIZE(data)};

      sand_assert(SUCCESS ==
                 sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));
    }

    sand_log("Test return data");
    {
      SolAccountMeta arguments[] = {{accounts[ARGUMENT_INDEX].key, true, true}};
      uint8_t data[] = { SET_RETURN_DATA };
      uint8_t buf[100];

      const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAND_ARRAY_SIZE(arguments),
                                          data, SAND_ARRAY_SIZE(data)};

      // set some return data, so that the callee can check it is cleared
      sand_set_return_data((uint8_t[]){1, 2, 3, 4}, 4);

      sand_assert(SUCCESS ==
                 sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));

      SolPubkey setter;

      uint64_t ret = sand_get_return_data(data, sizeof(data), &setter);

      sand_assert(ret == sizeof(RETURN_DATA_VAL));

      sand_assert(sand_memcmp(data, RETURN_DATA_VAL, sizeof(RETURN_DATA_VAL)));
      sand_assert(SolPubkey_same(&setter, accounts[INVOKED_PROGRAM_INDEX].key));
    }

    sand_log("Test create_program_address");
    {
      uint8_t seed1[] = {'Y', 'o', 'u', ' ', 'p', 'a', 's', 's',
                         ' ', 'b', 'u', 't', 't', 'e', 'r'};
      const SolSignerSeed seeds1[] = {{seed1, SAND_ARRAY_SIZE(seed1)},
                                      {&bump_seed1, 1}};
      SolPubkey address;
      sand_assert(SUCCESS ==
                 sand_create_program_address(seeds1, SAND_ARRAY_SIZE(seeds1),
                                            params.program_id, &address));
      sand_assert(SolPubkey_same(&address, accounts[DERIVED_KEY1_INDEX].key));
    }

    sand_log("Test try_find_program_address");
    {
      uint8_t seed[] = {'Y', 'o', 'u', ' ', 'p', 'a', 's', 's',
                        ' ', 'b', 'u', 't', 't', 'e', 'r'};
      const SolSignerSeed seeds[] = {{seed, SAND_ARRAY_SIZE(seed)}};
      SolPubkey address;
      uint8_t bump_seed;
      sand_assert(SUCCESS == sand_try_find_program_address(
                                seeds, SAND_ARRAY_SIZE(seeds), params.program_id,
                                &address, &bump_seed));
      sand_assert(SolPubkey_same(&address, accounts[DERIVED_KEY1_INDEX].key));
      sand_assert(bump_seed == bump_seed1);
    }

    sand_log("Test derived signers");
    {
      sand_assert(!accounts[DERIVED_KEY1_INDEX].is_signer);
      sand_assert(!accounts[DERIVED_KEY2_INDEX].is_signer);
      sand_assert(!accounts[DERIVED_KEY3_INDEX].is_signer);

      SolAccountMeta arguments[] = {
          {accounts[INVOKED_PROGRAM_INDEX].key, false, false},
          {accounts[DERIVED_KEY1_INDEX].key, true, true},
          {accounts[DERIVED_KEY2_INDEX].key, true, false},
          {accounts[DERIVED_KEY3_INDEX].key, false, false}};
      uint8_t data[] = {DERIVED_SIGNERS, bump_seed2, bump_seed3};
      const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAND_ARRAY_SIZE(arguments),
                                          data, SAND_ARRAY_SIZE(data)};
      uint8_t seed1[] = {'Y', 'o', 'u', ' ', 'p', 'a', 's', 's',
                         ' ', 'b', 'u', 't', 't', 'e', 'r'};
      const SolSignerSeed seeds1[] = {{seed1, SAND_ARRAY_SIZE(seed1)},
                                      {&bump_seed1, 1}};
      const SolSignerSeeds signers_seeds[] = {{seeds1, SAND_ARRAY_SIZE(seeds1)}};
      sand_assert(SUCCESS == sand_invoke_signed(&instruction, accounts,
                                              SAND_ARRAY_SIZE(accounts),
                                              signers_seeds,
                                              SAND_ARRAY_SIZE(signers_seeds)));
    }

    sand_log("Test readonly with writable account");
    {
      SolAccountMeta arguments[] = {
          {accounts[INVOKED_ARGUMENT_INDEX].key, true, false}};
      uint8_t data[] = {VERIFY_WRITER};
      const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAND_ARRAY_SIZE(arguments),
                                          data, SAND_ARRAY_SIZE(data)};

      sand_assert(SUCCESS ==
                 sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));
    }

    sand_log("Test nested invoke");
    {
      sand_assert(SUCCESS == do_nested_invokes(4, accounts, params.ka_num));
    }

    sand_log("Test privilege deescalation");
    {
      sand_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
      sand_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
      SolAccountMeta arguments[] = {
          {accounts[INVOKED_ARGUMENT_INDEX].key, false, false}};
      uint8_t data[] = {VERIFY_PRIVILEGE_DEESCALATION};
      const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAND_ARRAY_SIZE(arguments),
                                          data, SAND_ARRAY_SIZE(data)};
      sand_assert(SUCCESS ==
                 sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));
    }

    sand_log("Verify data values are retained and updated");
    for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
      sand_assert(accounts[ARGUMENT_INDEX].data[i] == i);
    }
    for (int i = 0; i < accounts[INVOKED_ARGUMENT_INDEX].data_len; i++) {
      sand_assert(accounts[INVOKED_ARGUMENT_INDEX].data[i] == i);
    }

    sand_log("Verify data write before ro cpi call");
    {
      for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
        accounts[ARGUMENT_INDEX].data[i] = 0;
      }

      SolAccountMeta arguments[] = {
          {accounts[ARGUMENT_INDEX].key, false, false}};
      uint8_t data[] = {VERIFY_PRIVILEGE_DEESCALATION};
      const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                          arguments, SAND_ARRAY_SIZE(arguments),
                                          data, SAND_ARRAY_SIZE(data)};
      sand_assert(SUCCESS ==
                 sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));

      for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
        sand_assert(accounts[ARGUMENT_INDEX].data[i] == 0);
      }
    }
    break;
  }
  case TEST_PRIVILEGE_ESCALATION_SIGNER: {
    sand_log("Test privilege escalation signer");
    SolAccountMeta arguments[] = {
        {accounts[DERIVED_KEY3_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};
    sand_assert(SUCCESS ==
               sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));

    // Signer privilege escalation will always fail the whole transaction
    instruction.accounts[0].is_signer = true;
    sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts));
    break;
  }
  case TEST_PRIVILEGE_ESCALATION_WRITABLE: {
    sand_log("Test privilege escalation writable");
    SolAccountMeta arguments[] = {
        {accounts[DERIVED_KEY3_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};
    sand_assert(SUCCESS ==
               sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));

    // Writable privilege escalation will always fail the whole transaction
    instruction.accounts[0].is_writable = true;
    sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts));
    break;
  }
  case TEST_PPROGRAM_NOT_EXECUTABLE: {
    sand_log("Test program not executable");
    SolAccountMeta arguments[] = {
        {accounts[DERIVED_KEY3_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SolInstruction instruction = {accounts[ARGUMENT_INDEX].key, arguments,
                                        SAND_ARRAY_SIZE(arguments), data,
                                        SAND_ARRAY_SIZE(data)};
    return sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts));
  }
  case TEST_EMPTY_ACCOUNTS_SLICE: {
    sand_log("Empty accounts slice");

    SolAccountMeta arguments[] = {
        {accounts[INVOKED_ARGUMENT_INDEX].key, false, false}};
    uint8_t data[] = {};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};

    sand_assert(SUCCESS == sand_invoke(&instruction, 0, 0));
    break;
  }
  case TEST_CAP_SEEDS: {
    sand_log("Test cap seeds");
    SolAccountMeta arguments[] = {};
    uint8_t data[] = {};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};
    uint8_t seed[] = {"seed"};
    const SolSignerSeed seeds[] = {
        {seed, SAND_ARRAY_SIZE(seed)}, {seed, SAND_ARRAY_SIZE(seed)},
        {seed, SAND_ARRAY_SIZE(seed)}, {seed, SAND_ARRAY_SIZE(seed)},
        {seed, SAND_ARRAY_SIZE(seed)}, {seed, SAND_ARRAY_SIZE(seed)},
        {seed, SAND_ARRAY_SIZE(seed)}, {seed, SAND_ARRAY_SIZE(seed)},
        {seed, SAND_ARRAY_SIZE(seed)}, {seed, SAND_ARRAY_SIZE(seed)},
        {seed, SAND_ARRAY_SIZE(seed)}, {seed, SAND_ARRAY_SIZE(seed)},
        {seed, SAND_ARRAY_SIZE(seed)}, {seed, SAND_ARRAY_SIZE(seed)},
        {seed, SAND_ARRAY_SIZE(seed)}, {seed, SAND_ARRAY_SIZE(seed)},
        {seed, SAND_ARRAY_SIZE(seed)},
    };
    const SolSignerSeeds signers_seeds[] = {{seeds, SAND_ARRAY_SIZE(seeds)}};
    sand_assert(SUCCESS == sand_invoke_signed(
                              &instruction, accounts, SAND_ARRAY_SIZE(accounts),
                              signers_seeds, SAND_ARRAY_SIZE(signers_seeds)));
    break;
  }
  case TEST_CAP_SIGNERS: {
    sand_log("Test cap signers");
    SolAccountMeta arguments[] = {};
    uint8_t data[] = {};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};
    uint8_t seed[] = {"seed"};
    const SolSignerSeed seed1[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed2[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed3[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed4[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed5[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed6[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed7[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed8[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed9[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed10[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed11[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed12[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed13[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed14[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed15[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed16[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeed seed17[] = {{seed, SAND_ARRAY_SIZE(seed)}};
    const SolSignerSeeds signers_seeds[] = {
        {seed1, SAND_ARRAY_SIZE(seed1)},   {seed2, SAND_ARRAY_SIZE(seed2)},
        {seed3, SAND_ARRAY_SIZE(seed3)},   {seed4, SAND_ARRAY_SIZE(seed4)},
        {seed5, SAND_ARRAY_SIZE(seed5)},   {seed6, SAND_ARRAY_SIZE(seed6)},
        {seed7, SAND_ARRAY_SIZE(seed7)},   {seed8, SAND_ARRAY_SIZE(seed8)},
        {seed9, SAND_ARRAY_SIZE(seed9)},   {seed10, SAND_ARRAY_SIZE(seed10)},
        {seed11, SAND_ARRAY_SIZE(seed11)}, {seed12, SAND_ARRAY_SIZE(seed12)},
        {seed13, SAND_ARRAY_SIZE(seed13)}, {seed14, SAND_ARRAY_SIZE(seed14)},
        {seed15, SAND_ARRAY_SIZE(seed15)}, {seed16, SAND_ARRAY_SIZE(seed16)},
        {seed17, SAND_ARRAY_SIZE(seed17)}};
    sand_assert(SUCCESS == sand_invoke_signed(
                              &instruction, accounts, SAND_ARRAY_SIZE(accounts),
                              signers_seeds, SAND_ARRAY_SIZE(signers_seeds)));
    break;
  }
  case TEST_ALLOC_ACCESS_VIOLATION: {
    sand_log("Test resize violation");
    SolAccountMeta arguments[] = {
        {accounts[FROM_INDEX].key, true, true},
        {accounts[DERIVED_KEY1_INDEX].key, true, true}};
    uint8_t data[4 + 8 + 8 + 32];
    *(uint64_t *)(data + 4) = 42;
    *(uint64_t *)(data + 4 + 8) = MAX_PERMITTED_DATA_INCREASE;
    sand_memcpy(data + 4 + 8 + 8, params.program_id, SIZE_PUBKEY);
    const SolInstruction instruction = {accounts[SYSTEM_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};
    uint8_t seed1[] = {'Y', 'o', 'u', ' ', 'p', 'a', 's', 's',
                       ' ', 'b', 'u', 't', 't', 'e', 'r'};
    const SolSignerSeed seeds1[] = {{seed1, SAND_ARRAY_SIZE(seed1)},
                                    {&bump_seed1, 1}};
    const SolSignerSeeds signers_seeds[] = {{seeds1, SAND_ARRAY_SIZE(seeds1)}};

    SolAccountInfo derived_account = {
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
    const SolAccountInfo invoke_accounts[] = {
        accounts[FROM_INDEX], accounts[SYSTEM_PROGRAM_INDEX], derived_account};
    sand_assert(SUCCESS ==
               sand_invoke_signed(&instruction,
                                 (const SolAccountInfo *)invoke_accounts, 3,
                                 signers_seeds, SAND_ARRAY_SIZE(signers_seeds)));
    break;
  }
  case TEST_INSTRUCTION_DATA_TOO_LARGE: {
    sand_log("Test instruction data too large");
    SolAccountMeta arguments[] = {};
    uint8_t *data = sand_calloc(1500, 1);
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, 1500};
    const SolSignerSeeds signers_seeds[] = {};
    sand_assert(SUCCESS == sand_invoke_signed(
                              &instruction, accounts, SAND_ARRAY_SIZE(accounts),
                              signers_seeds, SAND_ARRAY_SIZE(signers_seeds)));

    break;
  }
  case TEST_INSTRUCTION_META_TOO_LARGE: {
    sand_log("Test instruction meta too large");
    SolAccountMeta *arguments = sand_calloc(40, sizeof(SolAccountMeta));
    sand_log_64(0, 0, 0, 0, (uint64_t)arguments);
    sand_assert(0 != arguments);
    uint8_t data[] = {};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, 40, data,
                                        SAND_ARRAY_SIZE(data)};
    const SolSignerSeeds signers_seeds[] = {};
    sand_assert(SUCCESS == sand_invoke_signed(
                              &instruction, accounts, SAND_ARRAY_SIZE(accounts),
                              signers_seeds, SAND_ARRAY_SIZE(signers_seeds)));

    break;
  }
  case TEST_RETURN_ERROR: {
    sand_log("Test return error");
    SolAccountMeta arguments[] = {{accounts[ARGUMENT_INDEX].key, false, true}};
    uint8_t data[] = {RETURN_ERROR};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};

    sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts));
    break;
  }
  case TEST_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER: {
    sand_log("Test privilege deescalation escalation signer");
    sand_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    sand_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    SolAccountMeta arguments[] = {
        {accounts[INVOKED_PROGRAM_INDEX].key, false, false},
        {accounts[INVOKED_ARGUMENT_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_SIGNER};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};
    sand_assert(SUCCESS ==
               sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));
    break;
  }
  case TEST_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE: {
    sand_log("Test privilege deescalation escalation writable");
    sand_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    sand_assert(true == accounts[INVOKED_ARGUMENT_INDEX].is_writable);
    SolAccountMeta arguments[] = {
        {accounts[INVOKED_PROGRAM_INDEX].key, false, false},
        {accounts[INVOKED_ARGUMENT_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_DEESCALATION_ESCALATION_WRITABLE};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};
    sand_assert(SUCCESS ==
               sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));
    break;
  }
  case TEST_WRITABLE_DEESCALATION_WRITABLE: {
    sand_log("Test writable deescalation");
    uint8_t buffer[10];
    for (int i = 0; i < 10; i++) {
      buffer[i] = accounts[INVOKED_ARGUMENT_INDEX].data[i];
    }
    SolAccountMeta arguments[] = {
        {accounts[INVOKED_ARGUMENT_INDEX].key, false, false}};
    uint8_t data[] = {WRITE_ACCOUNT, 10};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};
    sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts));

    for (int i = 0; i < 10; i++) {
      sand_assert(buffer[i] == accounts[INVOKED_ARGUMENT_INDEX].data[i]);
    }
    break;
  }
  case TEST_NESTED_INVOKE_TOO_DEEP: {
    do_nested_invokes(5, accounts, params.ka_num);
    break;
  }
  case TEST_EXECUTABLE_LAMPORTS: {
    sand_log("Test executable lamports");
    accounts[ARGUMENT_INDEX].executable = true;
    *accounts[ARGUMENT_INDEX].lamports -= 1;
    *accounts[DERIVED_KEY1_INDEX].lamports +=1;
    SolAccountMeta arguments[] = {
      {accounts[ARGUMENT_INDEX].key, true, false},
      {accounts[DERIVED_KEY1_INDEX].key, true, false},
    };
    uint8_t data[] = {ADD_LAMPORTS, 0, 0, 0};
    SolPubkey program_id;
    sand_memcpy(&program_id, params.program_id, sizeof(SolPubkey));
    const SolInstruction instruction = {&program_id,
					arguments, SAND_ARRAY_SIZE(arguments),
					data, SAND_ARRAY_SIZE(data)};
    sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts));
    *accounts[ARGUMENT_INDEX].lamports += 1;
    break;
  }
  case TEST_CALL_PRECOMPILE: {
    sand_log("Test calling precompile from cpi");
    SolAccountMeta arguments[] = {};
    uint8_t data[] = {};
    const SolInstruction instruction = {accounts[ED25519_PROGRAM_INDEX].key,
					arguments, SAND_ARRAY_SIZE(arguments),
					data, SAND_ARRAY_SIZE(data)};
    sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts));
    break;
  }
  case ADD_LAMPORTS: {
    *accounts[0].lamports += 1;
     break;
  }
  case TEST_RETURN_DATA_TOO_LARGE: {
    sand_log("Test setting return data too long");
    // The actual buffer doesn't matter, just pass null
    sand_set_return_data(NULL, 1027);
    break;
  }
  case TEST_DUPLICATE_PRIVILEGE_ESCALATION_SIGNER: {
    sand_log("Test duplicate privilege escalation signer");
    SolAccountMeta arguments[] = {
        {accounts[DERIVED_KEY3_INDEX].key, false, false},
        {accounts[DERIVED_KEY3_INDEX].key, false, false},
        {accounts[DERIVED_KEY3_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};
    sand_assert(SUCCESS ==
               sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));

    // Signer privilege escalation will always fail the whole transaction
    instruction.accounts[1].is_signer = true;
    sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts));
    break;
  }
  case TEST_DUPLICATE_PRIVILEGE_ESCALATION_WRITABLE: {
    sand_log("Test duplicate privilege escalation writable");
    SolAccountMeta arguments[] = {
        {accounts[DERIVED_KEY3_INDEX].key, false, false},
        {accounts[DERIVED_KEY3_INDEX].key, false, false},
        {accounts[DERIVED_KEY3_INDEX].key, false, false}};
    uint8_t data[] = {VERIFY_PRIVILEGE_ESCALATION};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SAND_ARRAY_SIZE(arguments),
                                        data, SAND_ARRAY_SIZE(data)};
    sand_assert(SUCCESS ==
               sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts)));

    // Writable privilege escalation will always fail the whole transaction
    instruction.accounts[1].is_writable = true;
    sand_invoke(&instruction, accounts, SAND_ARRAY_SIZE(accounts));
    break;
  }

  default:
    sand_panic();
  }

  return SUCCESS;
}
