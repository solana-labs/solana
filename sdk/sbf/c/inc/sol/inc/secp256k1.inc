#pragma once
/**
 * @brief Solana secp256k1 system call
 */

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/** Length of a secp256k1 recover input hash */
#define SECP256K1_RECOVER_HASH_LENGTH 32
/** Length of a secp256k1 input signature */
#define SECP256K1_RECOVER_SIGNATURE_LENGTH 64
/** Length of a secp256k1 recover result */
#define SECP256K1_RECOVER_RESULT_LENGTH 64

/** The hash provided to a sol_secp256k1_recover is invalid */
#define SECP256K1_RECOVER_ERROR_INVALID_HASH 1
/** The recovery_id provided to a sol_secp256k1_recover is invalid */
#define SECP256K1_RECOVER_ERROR_INVALID_RECOVERY_ID 2
/** The signature provided to a sol_secp256k1_recover is invalid */
#define SECP256K1_RECOVER_ERROR_INVALID_SIGNATURE 3

/**
 * Recover public key from a signed message.
 *
 * @param hash Hashed message
 * @param recovery_id Tag used for public key recovery from signatures. Can be 0 or 1
 * @param signature An ECDSA signature
 * @param result 64 byte array to hold the result. A recovered public key
 * @return 0 if executed successfully
 */
@SYSCALL uint64_t sol_secp256k1_recover(const uint8_t *, uint64_t, const uint8_t *, uint8_t *);

#ifdef __cplusplus
}
#endif

/**@}*/
