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
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/bpf/c/inc/sol/inc/alt_bn128.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_alt_bn128_addition(
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);
#else
typedef uint64_t(*sol_alt_bn128_addition_pointer_type)(
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);
static uint64_t sol_alt_bn128_addition(
        const uint8_t *input arg1,
        const uint64_t input_size arg2,
        uint8_t *result
 arg3) {
  sol_alt_bn128_addition_pointer_type sol_alt_bn128_addition_pointer = (sol_alt_bn128_addition_pointer_type) 334484080;
  return sol_alt_bn128_addition_pointer(arg1, arg2, arg3);
}
#endif

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
 /* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/bpf/c/inc/sol/inc/alt_bn128.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_alt_bn128_multiplication(
         const uint8_t *input,
         const uint64_t input_size,
         uint8_t *result
 );
#else
typedef uint64_t(*sol_alt_bn128_multiplication_pointer_type)(
         const uint8_t *input,
         const uint64_t input_size,
         uint8_t *result
 );
static uint64_t sol_alt_bn128_multiplication(
         const uint8_t *input arg1,
         const uint64_t input_size arg2,
         uint8_t *result
  arg3) {
  sol_alt_bn128_multiplication_pointer_type sol_alt_bn128_multiplication_pointer = (sol_alt_bn128_multiplication_pointer_type) 434091388;
  return sol_alt_bn128_multiplication_pointer(arg1, arg2, arg3);
}
#endif

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
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/bpf/c/inc/sol/inc/alt_bn128.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_alt_bn128_pairing(
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);
#else
typedef uint64_t(*sol_alt_bn128_pairing_pointer_type)(
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);
static uint64_t sol_alt_bn128_pairing(
        const uint8_t *input arg1,
        const uint64_t input_size arg2,
        uint8_t *result
 arg3) {
  sol_alt_bn128_pairing_pointer_type sol_alt_bn128_pairing_pointer = (sol_alt_bn128_pairing_pointer_type) 4165382592;
  return sol_alt_bn128_pairing_pointer(arg1, arg2, arg3);
}
#endif


#ifdef __cplusplus
}
#endif

/**@}*/
