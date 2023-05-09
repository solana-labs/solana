#pragma once
/**
 * @brief Solana big_mod_exp system call
**/

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Big integer modular exponentiation
 *
 * @param bytes Pointer to BigModExpParam struct
 * @param result 32 byte array to hold the result
 * @return 0 if executed successfully
 */
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/bpf/c/inc/sol/inc/big_mod_exp.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_big_mod_exp(const uint8_t *, uint8_t *);
#else
typedef uint64_t(*sol_big_mod_exp_pointer_type)(const uint8_t *, uint8_t *);
static uint64_t sol_big_mod_exp(const uint8_t * arg1, uint8_t * arg2) {
  sol_big_mod_exp_pointer_type sol_big_mod_exp_pointer = (sol_big_mod_exp_pointer_type) 2014202901;
  return sol_big_mod_exp_pointer(arg1, arg2);
}
#endif

#ifdef __cplusplus
}
#endif

/**@}*/
