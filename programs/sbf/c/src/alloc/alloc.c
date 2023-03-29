/**
 * @brief Example C-based SBF sanity rogram that prints out the parameters
 * passed to it
 */
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  {
    // Confirm large allocation fails
    void *ptr = sol_calloc(1, UINT64_MAX);
    if (ptr != NULL) {
      sol_log("Error: Alloc of very large type should fail");
      sol_panic();
    }
  }

  {
    // Confirm large allocation fails
    void *ptr = sol_calloc(UINT64_MAX, 1);
    if (ptr != NULL) {
      sol_log("Error: Alloc of very large number of items should fail");
      sol_panic();
    }
  }

  {
    // Test modest allocation and de-allocation
    void *ptr = sol_calloc(1, 100);
    if (ptr == NULL) {
      sol_log("Error: Alloc of 100 bytes failed");
      sol_panic();
    }
    sol_free(ptr);
  }

  {
    // Test allocated memory read and write

    const uint64_t iters = 100;
    uint8_t *ptr = sol_calloc(1, iters);
    if (ptr == NULL) {
      sol_log("Error: Alloc failed");
      sol_panic();
    }
    for (uint64_t i = 0; i < iters; i++) {
      *(ptr + i) = i;
    }
    for (uint64_t i = 0; i < iters; i++) {
      sol_assert(*(ptr + i) == i);
    }
    sol_assert(*(ptr + 42) == 42);
    sol_free(ptr);
  }

  // Alloc to exhaustion

  for (uint64_t i = 0; i < 31; i++) {
    uint8_t *ptr = sol_calloc(1024, 1);
    if (ptr == NULL) {
      sol_log("large alloc failed");
      sol_panic();
    }
  }
  for (uint64_t i = 0; i < 760; i++) {
    uint8_t *ptr = sol_calloc(1, 1);
    if (ptr == NULL) {
      sol_log("small alloc failed");
      sol_panic();
    }
  }
  uint8_t *ptr = sol_calloc(1, 1);
  if (ptr != NULL) {
    sol_log("final alloc did not fail");
    sol_panic();
  }

  return SUCCESS;
}
