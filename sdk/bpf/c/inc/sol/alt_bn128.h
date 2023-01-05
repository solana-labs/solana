#pragma once
/**
 * @brief Solana bn128 elliptic curve addition, multiplication, and pairing
**/

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Addition on elliptic curves alt_bn128
 *
 * @param group_op ...
 * @param input ...
 * @param input_size ...
 * @param result 64 byte array to hold the result. ...
 * @return 0 if executed successfully
 */
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/sbf/c/inc/sol/inc/alt_bn128.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_alt_bn128(
        const uint64_t *group_op,
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);
#else
typedef uint64_t(*sol_alt_bn128_pointer_type)(
        const uint64_t *group_op,
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);
static uint64_t sol_alt_bn128(
        const uint64_t *group_op arg1,
        const uint8_t *input arg2,
        const uint64_t input_size arg3,
        uint8_t *result
 arg4) {
  sol_alt_bn128_pointer_type sol_alt_bn128_pointer = (sol_alt_bn128_pointer_type) 2551807235;
  return sol_alt_bn128_pointer(arg1, arg2, arg3, arg4);
}
#endif

#ifdef __cplusplus
}
#endif

/**@}*/
