#pragma once
/**
 * @brief Solana poseidon system call
**/

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Length of a Poseidon hash result
 */
#define POSEIDON_RESULT_LENGTH 32

/**
 * Poseidon
 *
 * @param bytes Array of byte arrays
 * @param bytes_len Number of byte arrays
 * @param result 32 byte array to hold the result
 */
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/sbf/c/inc/sol/inc/poseidon.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_poseidon(const SolBytes *, int, uint8_t *);
#else
typedef uint64_t(*sol_poseidon_pointer_type)(const SolBytes *, int, uint8_t *);
static uint64_t sol_poseidon(const SolBytes * arg1, int arg2, uint8_t * arg3) {
  sol_poseidon_pointer_type sol_poseidon_pointer = (sol_poseidon_pointer_type) 3298065441;
  return sol_poseidon_pointer(arg1, arg2, arg3);
}
#endif

#ifdef __cplusplus
}
#endif

/**@}*/
