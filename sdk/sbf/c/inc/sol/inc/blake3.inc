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
@SYSCALL uint64_t sol_blake3(const SolBytes *, int, const uint8_t *);

#ifdef __cplusplus
}
#endif

/**@}*/
