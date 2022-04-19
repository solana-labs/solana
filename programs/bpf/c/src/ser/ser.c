/**
 * @brief Example C-based BPF sanity rogram that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  SolAccountInfo ka[2];
  SolParameters params = (SolParameters){.ka = ka};

  sand_log(__FILE__);

  if (!sand_deserialize(input, &params, SAND_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }

  char ka_data[] = {1, 2, 3};
  SolPubkey ka_owner;
  sand_memset(ka_owner.x, 0, SIZE_PUBKEY); // set to system program

  sand_assert(params.ka_num == 2);
  for (int i = 0; i < 2; i++) {
    sand_assert(*params.ka[i].lamports == 42);
    sand_assert(!sand_memcmp(params.ka[i].data, ka_data, 4));
    sand_assert(SolPubkey_same(params.ka[i].owner, &ka_owner));
    sand_assert(params.ka[i].is_signer == false);
    sand_assert(params.ka[i].is_writable == false);
    sand_assert(params.ka[i].executable == false);
  }

  char data[] = {4, 5, 6, 7};
  sand_assert(params.data_len = 4);
  sand_assert(!sand_memcmp(params.data, data, 4));

  return SUCCESS;
}
