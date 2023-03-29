#pragma once
/**
 * @brief Solana keccak system call
**/

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Length of a Keccak hash result
 */
#define KECCAK_RESULT_LENGTH 32

/**
 * Keccak
 *
 * @param bytes Array of byte arrays
 * @param bytes_len Number of byte arrays
 * @param result 32 byte array to hold the result
 */
@SYSCALL uint64_t sol_keccak256(const SolBytes *, int, uint8_t *);

#ifdef __cplusplus
}
#endif

/**@}*/
