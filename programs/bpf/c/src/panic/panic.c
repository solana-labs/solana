/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>

extern bool entrypoint(const uint8_t *input) {
  sol_panic();
  return true;
}

