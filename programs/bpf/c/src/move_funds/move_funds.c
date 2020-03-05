/**
 * @brief Example C-based BPF program that moves funds from one account to
 * another
 */
#include <solana_sdk.h>

/**
 * Number of SolKeyedAccount expected. The program should bail if an
 * unexpected number of accounts are passed to the program's entrypoint
 */
#define NUM_KA 3

extern uint64_t entrypoint(const uint8_t *input) {
  SolKeyedAccount ka[NUM_KA];
  SolParameters params = (SolParameters) { .ka = ka };

  if (!sol_deserialize(input, &params, SOL_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }

  if (!params.ka[0].is_signer) {
    sol_log("Transaction not signed by key 0");
    return ERROR_MISSING_REQUIRED_SIGNATURES;
  }

  int64_t lamports = *(int64_t *)params.data;
  if (*params.ka[0].lamports >= lamports) {
    *params.ka[0].lamports -= lamports;
    *params.ka[2].lamports += lamports;
    // sol_log_64(0, 0, *ka[0].lamports, *ka[2].lamports, lamports);
  } else {
    // sol_log_64(0, 0, 0xFF, *ka[0].lamports, lamports);
  }
  return SUCCESS;
}
