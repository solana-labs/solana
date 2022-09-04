#pragma once
/**
 * @brief Solana bn128 elliptic curve addition, multiplication, and pairing
**/

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Length of a Addition on elliptic curves alt_bn128 result
 *
 */
#define ALT_BN128_ADDITION_OUTPUT_LEN 64

/**
 * Addition on elliptic curves alt_bn128
 *
 * @param input ...
 * @param input_size ...
 * @param result 64 byte array to hold the result. ...
 * @return 0 if executed successfully
 */
@SYSCALL uint64_t sol_alt_bn128_addition(
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);

/**
 * Length of a Multiplication on elliptic curves alt_bn128 result
 *
 */
#define ALT_BN128_MULTIPLICATION_OUTPUT_LEN 64

/**
 * Multiplication on elliptic curves alt_bn128
 *
 * @param input ...
 * @param input_size ...
 * @param result 64 byte array to hold the result. ...
 * @return 0 if executed successfully
 */
 @SYSCALL uint64_t sol_alt_bn128_multiplication(
         const uint8_t *input,
         const uint64_t input_size,
         uint8_t *result
 );

 /**
  * Length of a Pairing on elliptic curves alt_bn128 result
  *
  */
#define ALT_BN128_PAIRING_OUTPUT_LEN 32

/**
 * Pairing on elliptic curves alt_bn128
 *
 * @param input ...
 * @param input_size ...
 * @param result 64 byte array to hold the result. ...
 * @return 0 if executed successfully
 */
@SYSCALL uint64_t sol_alt_bn128_pairing(
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);


#ifdef __cplusplus
}
#endif

/**@}*/
