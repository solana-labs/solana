/**
 * @brief test program that generates SBF PC relative call instructions
 */
#include <solana_sdk.h>

void __attribute__ ((noinline)) helper() {
  sol_log(__func__);
}

extern uint64_t entrypoint(const uint8_t *input) {
  sol_log(__func__);
  helper();
  return SUCCESS;
}
