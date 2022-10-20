#pragma once
/**
 * @brief Solana sha system call
 */

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Length of a sha256 hash result
 */
#define SHA256_RESULT_LENGTH 32

/**
 * Sha256
 *
 * @param bytes Array of byte arrays
 * @param bytes_len Number of byte arrays
 * @param result 32 byte array to hold the result
 */
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/sbf/c/inc/sol/inc/sha.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_sha256(const SolBytes *, int, uint8_t *);
#else
typedef uint64_t(*sol_sha256_pointer_type)(const SolBytes *, int, uint8_t *);
static uint64_t sol_sha256(const SolBytes * arg1, int arg2, uint8_t * arg3) {
  sol_sha256_pointer_type sol_sha256_pointer = (sol_sha256_pointer_type) 301243782;
  return sol_sha256_pointer(arg1, arg2, arg3);
}
#endif

#ifdef __cplusplus
}
#endif

/**@}*/
