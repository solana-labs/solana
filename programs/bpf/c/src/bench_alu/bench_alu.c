/**
 * @brief Benchmark program that does work
 *
 * Counts Armstrong Numbers between 1 and x
 */

#include <solana_sdk.h>

extern bool entrypoint(const uint8_t *input) {
  uint64_t x = *(uint64_t *) input;
  uint64_t *result = (uint64_t *) input + 1;
  uint64_t count = 0;

  for (int i = 1; i <= x; i++) {
    uint64_t temp = i;
    uint64_t num = 0;
    while (temp != 0) {
      uint64_t rem = (temp % 10);
      num += rem * rem * rem;
      temp /= 10;
    }
    if (i == num) {
      count++;
    }
  }

  sol_log_64(x, count, 0, 0, 0);
  *result = count;
  return true;
}
