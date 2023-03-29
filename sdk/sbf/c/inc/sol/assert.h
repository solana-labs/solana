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
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/sbf/c/inc/sol/inc/assert.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
void sol_panic_(const char *, uint64_t, uint64_t, uint64_t);
#else
typedef void(*sol_panic__pointer_type)(const char *, uint64_t, uint64_t, uint64_t);
static void sol_panic_(const char * arg1, uint64_t arg2, uint64_t arg3, uint64_t arg4) {
  sol_panic__pointer_type sol_panic__pointer = (sol_panic__pointer_type) 1751159739;
  sol_panic__pointer(arg1, arg2, arg3, arg4);
}
#endif
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
