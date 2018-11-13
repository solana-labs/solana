/**
 * @brief Benchmark program that does work
 *
 * Counts Armstrong Numbers between 1 and x
 */

#include <solana_sdk.h>

#define BPF_TRACE_PRINTK_IDX 6
static int (*sol_print)(
  uint64_t,
  uint64_t,
  uint64_t,
  uint64_t,
  uint64_t
) = (void *)BPF_TRACE_PRINTK_IDX;

extern bool entrypoint(const uint8_t *input) {
  uint64_t x = *(uint64_t *)input;
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

  return count;
}
