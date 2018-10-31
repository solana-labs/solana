/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */

#include <sol_bpf.h>

/**
 * Number of SolKeyedAccounts expected. The program should bail if an
 * unexpected number of accounts are passed to the program's entrypoint
 */
#define NUM_KA 1

extern bool entrypoint(const uint8_t *input) {
  SolKeyedAccounts ka[NUM_KA];
  uint8_t *data;
  uint64_t data_len;

  if (!sol_deserialize(input, NUM_KA, ka, &data, &data_len)) {
    return false;
  }
  sol_print_params(NUM_KA, ka, data, data_len);
  return true;
}
