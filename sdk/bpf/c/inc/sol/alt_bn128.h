#pragma once
/**
 * @brief Solana alt_bn128 system calls
 */

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Addition on elliptic curves alt_bn128
 *
 * @param input ...
 * @param input_size ...
 * @param result 64 byte array to hold the result. ...
 * @return 0 if executed successfully
 */
uint64_t sol_alt_bn128_addition(
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);

/**
 * Multiplication on elliptic curves alt_bn128
 *
 * @param input ...
 * @param input_size ...
 * @param result 64 byte array to hold the result. ...
 * @return 0 if executed successfully
 */
uint64_t sol_alt_bn128_multiplication(
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);

/**
 * Pairing on elliptic curves alt_bn128
 *
 * @param input ...
 * @param input_size ...
 * @param result 64 byte array to hold the result. ...
 * @return 0 if executed successfully
 */
uint64_t sol_alt_bn128_pairing(
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);

#ifdef __cplusplus
}
#endif

/**@}*/
