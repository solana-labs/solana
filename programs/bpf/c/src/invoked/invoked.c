/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  SolAccountInfo accounts[4];
  SolParameters params = (SolParameters){.ka = accounts};

  if (!sol_deserialize(input, &params, 0)) {
    return ERROR_INVALID_ARGUMENT;
  }

  switch (params.data[0]) {
  case (0): {
    sol_log("verify data translations");

    static const int ARGUMENT_INDEX = 0;
    static const int INVOKED_ARGUMENT_INDEX = 1;
    static const int INVOKED_PROGRAM_INDEX = 2;
    static const int INVOKED_PROGRAM_DUP_INDEX = 3;
    sol_assert(sol_deserialize(input, &params, 4));

    SolPubkey bpf_loader_id =
        (SolPubkey){.x = {2,   168, 246, 145, 78,  136, 161, 107, 189, 35,  149,
                          133, 95,  100, 4,   217, 180, 244, 86,  183, 130, 27,
                          176, 20,  87,  73,  66,  140, 0,   0,   0,   0}};

    for (int i = 0; i < params.data_len; i++) {
      sol_assert(params.data[i] == i);
    }
    sol_assert(params.ka_num == 4);

    sol_assert(*accounts[ARGUMENT_INDEX].lamports == 42);
    sol_assert(accounts[ARGUMENT_INDEX].data_len == 100);
    sol_assert(accounts[ARGUMENT_INDEX].is_signer);
    sol_assert(accounts[ARGUMENT_INDEX].is_writable);
    sol_assert(accounts[ARGUMENT_INDEX].rent_epoch == 1);
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
    sol_assert(accounts[INVOKED_ARGUMENT_INDEX].rent_epoch == 1);
    sol_assert(!accounts[INVOKED_ARGUMENT_INDEX].executable);

    sol_assert(
        SolPubkey_same(accounts[INVOKED_PROGRAM_INDEX].key, params.program_id))
        sol_assert(SolPubkey_same(accounts[INVOKED_PROGRAM_INDEX].owner,
                                   &bpf_loader_id));
    sol_assert(!accounts[INVOKED_PROGRAM_INDEX].is_signer);
    sol_assert(!accounts[INVOKED_PROGRAM_INDEX].is_writable);
    sol_assert(accounts[INVOKED_PROGRAM_INDEX].rent_epoch == 1);
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
  case (1): {
    sol_log("reutrn error");
    return 42;
  }
  case (2): {
    sol_log("verify derived signers");
    static const int DERIVED_KEY_INDEX = 0;
    static const int DERIVED_KEY2_INDEX = 1;
    sol_assert(sol_deserialize(input, &params, 2));

    sol_assert(accounts[DERIVED_KEY_INDEX].is_signer);
    sol_assert(accounts[DERIVED_KEY2_INDEX].is_signer);
    break;
  }
  case (3): {
    sol_log("verify writable");
    static const int ARGUMENT_INDEX = 0;
    sol_assert(sol_deserialize(input, &params, 1));

    sol_assert(accounts[ARGUMENT_INDEX].is_writable);
    break;
  }
  case (4): {
    sol_log("invoke");

    static const int INVOKED_ARGUMENT_INDEX = 0;
    static const int ARGUMENT_INDEX = 1;
    static const int DERIVED_KEY_INDEX = 2;
    sol_assert(sol_deserialize(input, &params, 3));

    sol_assert(accounts[INVOKED_ARGUMENT_INDEX].is_signer);
    sol_assert(accounts[ARGUMENT_INDEX].is_signer);
    sol_assert(accounts[DERIVED_KEY_INDEX].is_signer);

    *accounts[INVOKED_ARGUMENT_INDEX].lamports -= 1;
    *accounts[ARGUMENT_INDEX].lamports += 1;

    sol_log("Last invoke");
    for (int i = 0; i < accounts[INVOKED_ARGUMENT_INDEX].data_len; i++) {
      accounts[INVOKED_ARGUMENT_INDEX].data[i] = i;
    }
    break;
  }
  default:
    return ERROR_INVALID_INSTRUCTION_DATA;
  }
  return SUCCESS;
}
