/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */

#include "sol_bpf_c.h"

/**
 * Numer of SolKeyedAccounts expected. The program should bail if an
 * unexpected number of accounts are passed to the program's entrypoint
 */
#define NUM_KA 1

extern bool entrypoint(uint8_t *input) {
  SolKeyedAccounts ka[NUM_KA];
  uint8_t *data;
  uint64_t data_len;

  if (1 != sol_deserialize((uint8_t *)input, NUM_KA, ka, &data, &data_len)) {
    return false;
  }
  print_params(1, ka, data, data_len);
  return true;
}
