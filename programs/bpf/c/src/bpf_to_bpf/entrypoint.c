/**
 * @brief Example C-based BPF program that prints out the parameters
 * passed to it
 */
#include <safecoin_sdk.h>
#include "helper.h"

extern uint64_t entrypoint(const uint8_t *input) {
  safe_log(__FILE__);
  helper_function();
  safe_log(__FILE__);
  return SUCCESS;
}
