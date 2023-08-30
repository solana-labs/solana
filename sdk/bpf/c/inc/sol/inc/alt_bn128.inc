#pragma once
/**
 * @brief Solana bn128 elliptic curve addition, multiplication, and pairing
**/

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Output length for the add operation.
 */
#define ALT_BN128_ADDITION_OUTPUT_LEN 64

/**
 * Output length for the add operation.
 */
#define ALT_BN128_MULTIPLICATION_OUTPUT_LEN 64

/**
 * Output length for pairing operation.
 */
#define ALT_BN128_PAIRING_OUTPUT_LEN 32

/**
 * Add operation.
 */
#define ALT_BN128_ADD 0

/**
 * Subtraction operation.
 */
#define ALT_BN128_SUB 1

/**
 * Multiplication operation.
 */
#define ALT_BN128_MUL 2

/**
 * Pairing operation.
 */
#define ALT_BN128_PAIRING 3

/**
 * Addition on elliptic curves alt_bn128
 *
 * @param group_op ...
 * @param input ...
 * @param input_size ...
 * @param result 64 byte array to hold the result. ...
 * @return 0 if executed successfully
 */
@SYSCALL uint64_t sol_alt_bn128_group_op(
        const uint64_t group_op,
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);

#ifdef __cplusplus
}
#endif

/**@}*/
