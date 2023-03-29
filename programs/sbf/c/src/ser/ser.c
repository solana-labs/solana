/**
 * @brief Example C-based SBF sanity rogram that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  SolAccountInfo ka[2];
  SolParameters params = (SolParameters){.ka = ka};

  sol_log(__FILE__);

  if (!sol_deserialize(input, &params, SOL_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }

  char ka_data[] = {1, 2, 3};
  SolPubkey ka_owner;
  sol_memset(ka_owner.x, 0, SIZE_PUBKEY); // set to system program

  sol_assert(params.ka_num == 2);
  for (int i = 0; i < 2; i++) {
    sol_assert(*params.ka[i].lamports == 42);
    sol_assert(!sol_memcmp(params.ka[i].data, ka_data, 4));
    sol_assert(SolPubkey_same(params.ka[i].owner, &ka_owner));
    sol_assert(params.ka[i].is_signer == false);
    sol_assert(params.ka[i].is_writable == false);
    sol_assert(params.ka[i].executable == false);
  }

  char data[] = {4, 5, 6, 7};
  sol_assert(params.data_len = 4);
  sol_assert(!sol_memcmp(params.data, data, 4));

  return SUCCESS;
}
