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
@SYSCALL uint64_t sol_big_mod_exp(const uint8_t *, uint8_t *);

#ifdef __cplusplus
}
#endif

/**@}*/
