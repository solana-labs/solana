#pragma once
/**
 * @brief Solana string and memory system calls and utilities
 */

#include <sol/constants.h>
#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Copies memory
 */
static void *sol_memcpy(void *dst, const void *src, int len) {
  for (int i = 0; i < len; i++) {
    *((uint8_t *)dst + i) = *((const uint8_t *)src + i);
  }
  return dst;
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
  return b;
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
 * Alloc zero-initialized memory
 */
static void *sol_calloc(size_t nitems, size_t size) {
  // Bump allocator
  uint64_t* pos_ptr = (uint64_t*)HEAP_START_ADDRESS;

  uint64_t pos = *pos_ptr;
  if (pos == 0) {
      /** First time, set starting position */
      pos = HEAP_START_ADDRESS + HEAP_LENGTH;
  }

  uint64_t bytes = (uint64_t)(nitems * size);
  if (size == 0 ||
      !(nitems == 0 || size == 0) &&
      !(nitems == bytes / size)) {
    /** Overflow */
    return NULL;
  }
  if (pos < bytes) {
    /** Saturated */
    pos = 0;
  } else {
    pos -= bytes;
  }

  uint64_t align = size;
  align--;
  align |= align >> 1;
  align |= align >> 2;
  align |= align >> 4;
  align |= align >> 8;
  align |= align >> 16;
  align |= align >> 32;
  align++;
  pos &= ~(align - 1);
  if (pos < HEAP_START_ADDRESS + sizeof(uint8_t*)) {
      return NULL;
  }
  *pos_ptr = pos;
  return (void*)pos;
}

/**
 * Deallocates the memory previously allocated by sol_calloc
 */
static void sol_free(void *ptr) {
  // I'm a bump allocator, I don't free
}

#ifdef __cplusplus
}
#endif

/**@}*/
