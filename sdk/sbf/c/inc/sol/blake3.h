#pragma once
/**
 * @brief Solana Blake3 system call
 */

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Length of a Blake3 hash result
 */
#define BLAKE3_RESULT_LENGTH 32

/**
 * Blake3
 *
 * @param bytes Array of byte arrays
 * @param bytes_len Number of byte arrays
 * @param result 32 byte array to hold the result
 */
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/sbf/c/inc/sol/inc/blake3.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_blake3(const SolBytes *, int, const uint8_t *);
#else
typedef uint64_t(*sol_blake3_pointer_type)(const SolBytes *, int, const uint8_t *);
static uint64_t sol_blake3(const SolBytes * arg1, int arg2, const uint8_t * arg3) {
  sol_blake3_pointer_type sol_blake3_pointer = (sol_blake3_pointer_type) 390877474;
  return sol_blake3_pointer(arg1, arg2, arg3);
}
#endif

#ifdef __cplusplus
}
#endif

/**@}*/
