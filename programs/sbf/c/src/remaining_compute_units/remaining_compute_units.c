/**
 * @brief sol_remaining_compute_units Syscall test
 */
#include <solana_sdk.h>
#include <stdio.h>

extern uint64_t entrypoint(const uint8_t *input) {
  char buffer[200];

  int i = 0;
  for (; i < 100000; ++i) {
    if (i % 500 == 0) {
      uint64_t remaining = sol_remaining_compute_units();
      snprintf(buffer, 200, "remaining compute units: %d", (int)remaining);
      sol_log(buffer);
      if (remaining < 25000) {
        break;
      }
    }
  }

  return SUCCESS;
}
