/**
 * @brief Example C-based BPF program that moves funds from one account to
 * another
 */

#include <solana_sdk.h>

/**
 * Number of SolKeyedAccounts expected. The program should bail if an
 * unexpected number of accounts are passed to the program's entrypoint
 */
#define NUM_KA 3

extern bool entrypoint(const uint8_t *input) {
  SolKeyedAccounts ka[NUM_KA];
  const uint8_t *data;
  uint64_t data_len;

  if (!sol_deserialize(input, ka, NUM_KA, NULL, &data, &data_len, NULL)) {
    return false;
  }

  int64_t tokens = *(int64_t *)data;
  if (*ka[0].tokens >= tokens) {
    *ka[0].tokens -= tokens;
    *ka[2].tokens += tokens;
    // sol_log_64(0, 0, *ka[0].tokens, *ka[2].tokens, tokens);
  } else {
    // sol_log_64(0, 0, 0xFF, *ka[0].tokens, tokens);
  }
  return true;
}
