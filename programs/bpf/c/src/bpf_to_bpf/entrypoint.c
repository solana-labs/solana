/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>
#include "helper.h"

extern uint64_t entrypoint(const uint8_t *input) {
  sand_log(__FILE__);
  helper_function();
  sand_log(__FILE__);
  return SUCCESS;
}
