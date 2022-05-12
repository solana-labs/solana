#pragma once
/**
 * @brief Solana string and memory system calls and utilities
 */

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Copies memory
 */
static void sol_memcpy(void *dst, const void *src, int len) {
  for (int i = 0; i < len; i++) {
    *((uint8_t *)dst + i) = *((const uint8_t *)src + i);
  }
}

/**
 * Compares memory
 */
static int sol_memcmp(const void *s1, const void *s2, int n) {
  for (int i = 0; i < n; i++) {
    uint8_t diff = *((uint8_t *)s1 + i) - *((const uint8_t *)s2 + i);
    if (diff) {
      return diff;
    }
  }
  return 0;
}

/**
 * Fill a byte string with a byte value
 */
static void *sol_memset(void *b, int c, size_t len) {
  uint8_t *a = (uint8_t *) b;
  while (len > 0) {
    *a = c;
    a++;
    len--;
  }
}

/**
 * Find length of string
 */
static size_t sol_strlen(const char *s) {
  size_t len = 0;
  while (*s) {
    len++;
    s++;
  }
  return len;
}

/**
 * Internal memory alloc/free function
 */
#ifndef SOL_SBFV2
void* sol_alloc_free_(uint64_t, void *);
#else
typedef void*(*sol_alloc_free__pointer_type)(uint64_t, void *);
static void* sol_alloc_free_(uint64_t arg1, void * arg2) {
  sol_alloc_free__pointer_type sol_alloc_free__pointer = (sol_alloc_free__pointer_type) 2213547663;
  return sol_alloc_free__pointer(arg1, arg2);
}
#endif

/**
 * Alloc zero-initialized memory
 */
static void *sol_calloc(size_t nitems, size_t size) {
  return sol_alloc_free_(nitems * size, 0);
}

/**
 * Deallocates the memory previously allocated by sol_calloc
 */
static void sol_free(void *ptr) {
  (void) sol_alloc_free_(0, ptr);
}

#ifdef __cplusplus
}
#endif

/**@}*/
