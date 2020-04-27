/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>

#define MINT_INDEX 0
#define ARGUMENT_INDEX 1
#define INVOKED_PROGRAM_INDEX 2
#define INVOKED_ARGUMENT_INDEX 3
#define INVOKED_PROGRAM_DUP_INDEX 4
#define ARGUMENT_DUP_INDEX 5
#define DERIVED_KEY_INDEX 6
#define DERIVED_KEY2_INDEX 7

extern uint64_t entrypoint(const uint8_t *input) {
  sol_log("Invoke C program");

  SolAccountInfo accounts[8];
  SolParameters params = (SolParameters){.ka = accounts};

  if (!sol_deserialize(input, &params, SOL_ARRAY_SIZE(accounts))) {
    return ERROR_INVALID_ARGUMENT;
  }

  sol_log("Test data translation");
  {
    for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
      accounts[ARGUMENT_INDEX].data[i] = i;
    }

    SolAccountMeta arguments[] = {
        {accounts[ARGUMENT_INDEX].key, true, true},
        {accounts[INVOKED_ARGUMENT_INDEX].key, true, true},
        {accounts[INVOKED_PROGRAM_INDEX].key, false, false},
        {accounts[INVOKED_PROGRAM_DUP_INDEX].key, false, false}};
    uint8_t data[] = {0, 1, 2, 3, 4, 5};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, 4, data, 6};

    sol_assert(SUCCESS == sol_invoke(&instruction, accounts,
                                                  SOL_ARRAY_SIZE(accounts)));
  }

  sol_log("Test return error");
  {
    SolAccountMeta arguments[] = {{accounts[ARGUMENT_INDEX].key, true, true}};
    uint8_t data[] = {1};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SOL_ARRAY_SIZE(arguments),
                                        data, SOL_ARRAY_SIZE(data)};

    sol_assert(42 == sol_invoke(&instruction, accounts,
                                             SOL_ARRAY_SIZE(accounts)));
  }

  sol_log("Test derived signers");
  {
    SolAccountMeta arguments[] = {
        {accounts[DERIVED_KEY_INDEX].key, true, true},
        {accounts[DERIVED_KEY2_INDEX].key, false, true}};
    uint8_t data[] = {2};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SOL_ARRAY_SIZE(arguments),
                                        data, SOL_ARRAY_SIZE(data)};
    const SolSignerSeed seeds1[] = {{"Lil'", 4}, {"Bits", 4}};
    const SolSignerSeed seeds2[] = {{"Gar Ma Nar Nar", 14}};
    const SolSignerSeeds signers_seeds[] = {{seeds1, SOL_ARRAY_SIZE(seeds1)},
                                            {seeds2, SOL_ARRAY_SIZE(seeds2)}};

    sol_assert(SUCCESS == sol_invoke_signed(
                              &instruction, accounts, SOL_ARRAY_SIZE(accounts),
                              signers_seeds, SOL_ARRAY_SIZE(signers_seeds)));
  }

  sol_log("Test readonly with writable account");
  {
    SolAccountMeta arguments[] = {
        {accounts[INVOKED_ARGUMENT_INDEX].key, true, false}};
    uint8_t data[] = {3};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SOL_ARRAY_SIZE(arguments),
                                        data, SOL_ARRAY_SIZE(data)};

    sol_assert(SUCCESS == sol_invoke(&instruction, accounts,
                                                  SOL_ARRAY_SIZE(accounts)));
  }

  sol_log("Test invoke");
  {
    sol_assert(accounts[ARGUMENT_INDEX].is_signer);
    sol_assert(!accounts[DERIVED_KEY_INDEX].is_signer);
    sol_assert(!accounts[DERIVED_KEY2_INDEX].is_signer);

    *accounts[ARGUMENT_INDEX].lamports -= 5;
    *accounts[INVOKED_ARGUMENT_INDEX].lamports += 5;

    SolAccountMeta arguments[] = {
        {accounts[INVOKED_ARGUMENT_INDEX].key, true, true},
        {accounts[ARGUMENT_INDEX].key, true, true},
        {accounts[DERIVED_KEY_INDEX].key, true, true}};
    uint8_t data[] = {4};
    const SolInstruction instruction = {accounts[INVOKED_PROGRAM_INDEX].key,
                                        arguments, SOL_ARRAY_SIZE(arguments),
                                        data, SOL_ARRAY_SIZE(data)};
    const SolSignerSeed seeds[] = {{"Lil'", 4}, {"Bits", 4}};
    const SolSignerSeeds signers_seeds[] = {{seeds, SOL_ARRAY_SIZE(seeds)}};

    sol_log("Fist invoke");
    sol_assert(SUCCESS == sol_invoke_signed(
                              &instruction, accounts, SOL_ARRAY_SIZE(accounts),
                              signers_seeds, SOL_ARRAY_SIZE(signers_seeds)));
    sol_log("2nd invoke from first program");
    sol_assert(SUCCESS == sol_invoke_signed(
                              &instruction, accounts, SOL_ARRAY_SIZE(accounts),
                              signers_seeds, SOL_ARRAY_SIZE(signers_seeds)));

    sol_assert(*accounts[ARGUMENT_INDEX].lamports == 42 - 5 + 1 + 1);
    sol_assert(*accounts[INVOKED_ARGUMENT_INDEX].lamports == 10 + 5 - 1 - 1);
  }

  sol_log("Verify data values are retained and updated");
  for (int i = 0; i < accounts[ARGUMENT_INDEX].data_len; i++) {
    sol_assert(accounts[ARGUMENT_INDEX].data[i] == i);
  }
  for (int i = 0; i < accounts[INVOKED_ARGUMENT_INDEX].data_len; i++) {
    sol_assert(accounts[INVOKED_ARGUMENT_INDEX].data[i] == i);
  }

  return SUCCESS;
}
