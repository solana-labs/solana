/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */

#include <solana_sdk.h>

extern bool entrypoint(const uint8_t *input) {
  SolKeyedAccounts ka[1];
  uint64_t ka_len;
  const uint8_t *data;
  uint64_t data_len;

  if (!sol_deserialize(input, ka, SOL_ARRAY_SIZE(ka), &ka_len, &data, &data_len)) {
    return false;
  }
  sol_log_params(ka_len, ka, data, data_len);
  return true;
}
