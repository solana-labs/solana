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
 * Configuration using the Barreto–Naehrig curve with an embedding degree of
 * 12, defined over a 254-bit prime field.
 *
 * Configuration Details:
 * - S-Box: x^5
 * - Width: 2 <= t <= 13
 * - Inputs: 1 <= n <= 12
 * - Full rounds: 8
 * - Partial rounds: Depending on width: [56, 57, 56, 60, 60, 63, 64, 63,
 *   60, 66, 60, 65]
 */
#define POSEIDON_PARAMETERS_BN254_X5 0

/**
 * Big-endian inputs and output
 */
#define POSEIDON_ENDIANNESS_BIG_ENDIAN 0

/**
 * Little-endian inputs and output
 */
#define POSEIDON_ENDIANNESS_LITTLE_ENDIAN 1

/**
 * Poseidon
 *
 * @param parameters Configuration parameters for the hash function
 * @param endianness Endianness of inputs and result
 * @param bytes Array of byte arrays
 * @param bytes_len Number of byte arrays
 * @param result 32 byte array to hold the result
 */
@SYSCALL uint64_t sol_poseidon(
  const uint64_t parameters,
  const uint64_t endianness,
  const SolBytes *bytes,
  const uint64_t bytes_len,
  uint8_t *result
);

#ifdef __cplusplus
}
#endif

/**@}*/
