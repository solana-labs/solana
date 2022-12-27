#pragma once
/**
 * @brief Solana Public key
 */

#include <sol/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Size of Public key in bytes
 */
#define SIZE_PUBKEY 32

/**
 * Public key
 */
typedef struct {
  uint8_t x[SIZE_PUBKEY];
} SolPubkey;

/**
 * Prints the hexadecimal representation of a public key
 *
 * @param key The public key to print
 */
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/sbf/c/inc/sol/inc/pubkey.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
void sol_log_pubkey(const SolPubkey *);
#else
typedef void(*sol_log_pubkey_pointer_type)(const SolPubkey *);
static void sol_log_pubkey(const SolPubkey * arg1) {
  sol_log_pubkey_pointer_type sol_log_pubkey_pointer = (sol_log_pubkey_pointer_type) 2129692874;
  sol_log_pubkey_pointer(arg1);
}
#endif

/**
 * Compares two public keys
 *
 * @param one First public key
 * @param two Second public key
 * @return true if the same
 */
static bool SolPubkey_same(const SolPubkey *one, const SolPubkey *two) {
  for (int i = 0; i < sizeof(*one); i++) {
    if (one->x[i] != two->x[i]) {
      return false;
    }
  }
  return true;
}

/**
 * Seed used to create a program address or passed to sol_invoke_signed
 */
typedef struct {
  const uint8_t *addr; /** Seed bytes */
  uint64_t len; /** Length of the seed bytes */
} SolSignerSeed;

/**
 * Seeds used by a signer to create a program address or passed to
 * sol_invoke_signed
 */
typedef struct {
  const SolSignerSeed *addr; /** An array of a signer's seeds */
  uint64_t len; /** Number of seeds */
} SolSignerSeeds;

/**
 * Create a program address
 *
 * @param seeds Seed bytes used to sign program accounts
 * @param seeds_len Length of the seeds array
 * @param program_id Program id of the signer
 * @param program_address Program address created, filled on return
 */
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/sbf/c/inc/sol/inc/pubkey.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_create_program_address(const SolSignerSeed *, int, const SolPubkey *, SolPubkey *);
#else
typedef uint64_t(*sol_create_program_address_pointer_type)(const SolSignerSeed *, int, const SolPubkey *, SolPubkey *);
static uint64_t sol_create_program_address(const SolSignerSeed * arg1, int arg2, const SolPubkey * arg3, SolPubkey * arg4) {
  sol_create_program_address_pointer_type sol_create_program_address_pointer = (sol_create_program_address_pointer_type) 2474062396;
  return sol_create_program_address_pointer(arg1, arg2, arg3, arg4);
}
#endif

/**
 * Try to find a program address and return corresponding bump seed
 *
 * @param seeds Seed bytes used to sign program accounts
 * @param seeds_len Length of the seeds array
 * @param program_id Program id of the signer
 * @param program_address Program address created, filled on return
 * @param bump_seed Bump seed required to create a valid program address
 */
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/sbf/c/inc/sol/inc/pubkey.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_try_find_program_address(const SolSignerSeed *, int, const SolPubkey *, SolPubkey *, uint8_t *);
#else
typedef uint64_t(*sol_try_find_program_address_pointer_type)(const SolSignerSeed *, int, const SolPubkey *, SolPubkey *, uint8_t *);
static uint64_t sol_try_find_program_address(const SolSignerSeed * arg1, int arg2, const SolPubkey * arg3, SolPubkey * arg4, uint8_t * arg5) {
  sol_try_find_program_address_pointer_type sol_try_find_program_address_pointer = (sol_try_find_program_address_pointer_type) 1213221432;
  return sol_try_find_program_address_pointer(arg1, arg2, arg3, arg4, arg5);
}
#endif

#ifdef SOL_TEST
/**
 * Stub functions when building tests
 */
#include <stdio.h>

void sol_log_pubkey(
  const SolPubkey *pubkey
) {
  printf("Program log: ");
  for (int i = 0; i < SIZE_PUBKEY; i++) {
    printf("%02 ", pubkey->x[i]);
  }
  printf("\n");
}

#endif

#ifdef __cplusplus
}
#endif

/**@}*/
