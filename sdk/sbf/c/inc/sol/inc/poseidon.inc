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
@SYSCALL uint64_t sol_poseidon(const SolBytes *, int, uint8_t *);

#ifdef __cplusplus
}
#endif

/**@}*/
