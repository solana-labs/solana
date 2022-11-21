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
@SYSCALL uint64_t sol_alt_bn128(
        const uint64_t *group_op,
        const uint8_t *input,
        const uint64_t input_size,
        uint8_t *result
);

#ifdef __cplusplus
}
#endif

/**@}*/
