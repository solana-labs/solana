/**
 * @brief Example C-based BPF sanity rogram that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  {
    // Confirm large allocation fails
    void *ptr = sand_calloc(1, UINT64_MAX);
    if (ptr != NULL) {
      sand_log("Error: Alloc of very larger buffer should fail");
      sand_panic();
    }
  }

  {
    // Confirm large allocation fails
    void *ptr = sand_calloc(UINT64_MAX, 1);
    if (ptr != NULL) {
      sand_log("Error: Alloc of very larger buffer should fail");
      sand_panic();
    }
  }

  {
    // Test modest allocation and de-allocation
    void *ptr = sand_calloc(1, 100);
    if (ptr == NULL) {
      sand_log("Error: Alloc of 100 bytes failed");
      sand_panic();
    }
    sand_free(ptr);
  }

  {
    // Test allocated memory read and write

    const uint64_t iters = 100;
    uint8_t *ptr = sand_calloc(1, iters);
    if (ptr == NULL) {
      sand_log("Error: Alloc failed");
      sand_panic();
    }
    for (int i = 0; i < iters; i++) {
      *(ptr + i) = i;
    }
    for (int i = 0; i < iters; i++) {
      sand_assert(*(ptr + i) == i);
    }
    sand_log_64(0x3, 0, 0, 0, *(ptr + 42));
    sand_assert(*(ptr + 42) == 42);
    sand_free(ptr);
  }

  return SUCCESS;
}
