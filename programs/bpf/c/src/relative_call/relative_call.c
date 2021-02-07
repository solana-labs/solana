/**
 * @brief test program that generates BPF PC relative call instructions
 */
#include <safecoin_sdk.h>

void __attribute__ ((noinline)) helper() {
  safe_log(__func__);
}

extern uint64_t entrypoint(const uint8_t *input) {
  safe_log(__func__);
  helper();
  return SUCCESS;
}
