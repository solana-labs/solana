#pragma once
/**
 * @brief Solana assert and panic utilities
 */

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif


/**
 * Panics
 *
 * Prints the line number where the panic occurred and then causes
 * the SBF VM to immediately halt execution. No accounts' data are updated
 */
@SYSCALL void sol_panic_(const char *, uint64_t, uint64_t, uint64_t);
#define sol_panic() sol_panic_(__FILE__, sizeof(__FILE__), __LINE__, 0)

/**
 * Asserts
 */
#define sol_assert(expr)  \
if (!(expr)) {          \
  sol_panic(); \
}

#ifdef SOL_TEST
/**
 * Stub functions when building tests
 */
#include <stdio.h>
#include <stdlib.h>

void sol_panic_(const char *file, uint64_t len, uint64_t line, uint64_t column) {
  printf("Panic in %s at %d:%d\n", file, line, column);
  abort();
}
#endif

#ifdef __cplusplus
}
#endif

/**@}*/
