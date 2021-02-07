/**
 * @brief Example C-based BPF sanity rogram that prints out the parameters
 * passed to it
 */
#include <safecoin_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  {
    // Confirm large allocation fails
    void *ptr = safe_calloc(1, UINT64_MAX); // TODO use max
    if (ptr != NULL) {
      safe_log("Error: Alloc of very larger buffer should fail");
      sol_panic();
    }
  }

  {
    // Confirm large allocation fails
    void *ptr = safe_calloc(18446744073709551615U, 1); // TODO use max
    if (ptr != NULL) {
      safe_log("Error: Alloc of very larger buffer should fail");
      sol_panic();
    }
  }

  {
    // Test modest allocation and de-allocation
    void *ptr = safe_calloc(1, 100);
    if (ptr == NULL) {
      safe_log("Error: Alloc of 100 bytes failed");
      sol_panic();
    }
    safe_free(ptr);
  }

  {
    // Test allocated memory read and write

    const uint64_t iters = 100;
    uint8_t *ptr = safe_calloc(1, iters);
    if (ptr == NULL) {
      safe_log("Error: Alloc failed");
      sol_panic();
    }
    for (int i = 0; i < iters; i++) {
      *(ptr + i) = i;
    }
    for (int i = 0; i < iters; i++) {
      safe_assert(*(ptr + i) == i);
    }
    safe_log_64(0x3, 0, 0, 0, *(ptr + 42));
    safe_assert(*(ptr + 42) == 42);
    safe_free(ptr);
  }

  return SUCCESS;
}
