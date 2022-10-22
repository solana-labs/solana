#pragma once
/**
 * @brief Solana Cross-Program Invocation
 */

#include <sol/types.h>
#include <sol/pubkey.h>
#include <sol/entrypoint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Maximum CPI instruction data size. 10 KiB was chosen to ensure that CPI
 * instructions are not more limited than transaction instructions if the size
 * of transactions is doubled in the future.
 */
static const uint64_t MAX_CPI_INSTRUCTION_DATA_LEN = 10240;

/**
 * Maximum CPI instruction accounts. 255 was chosen to ensure that instruction
 * accounts are always within the maximum instruction account limit for SBF
 * program instructions.
 */
static const uint8_t MAX_CPI_INSTRUCTION_ACCOUNTS = 255;

/**
 * Maximum number of account info structs that can be used in a single CPI
 * invocation. A limit on account info structs is effectively the same as
 * limiting the number of unique accounts. 128 was chosen to match the max
 * number of locked accounts per transaction (MAX_TX_ACCOUNT_LOCKS).
 */
static const uint16_t MAX_CPI_ACCOUNT_INFOS = 128;

/**
 * Account Meta
 */
typedef struct {
  SolPubkey *pubkey; /** An account's public key */
  bool is_writable; /** True if the `pubkey` can be loaded as a read-write account */
  bool is_signer; /** True if an Instruction requires a Transaction signature matching `pubkey` */
} SolAccountMeta;

/**
 * Instruction
 */
typedef struct {
  SolPubkey *program_id; /** Pubkey of the instruction processor that executes this instruction */
  SolAccountMeta *accounts; /** Metadata for what accounts should be passed to the instruction processor */
  uint64_t account_len; /** Number of SolAccountMetas */
  uint8_t *data; /** Opaque data passed to the instruction processor */
  uint64_t data_len; /** Length of the data in bytes */
} SolInstruction;

/**
 * Internal cross-program invocation function
 */
/* DO NOT MODIFY THIS GENERATED FILE. INSTEAD CHANGE sdk/sbf/c/inc/sol/inc/cpi.inc AND RUN `cargo run --bin gen-headers` */
#ifndef SOL_SBFV2
uint64_t sol_invoke_signed_c(
  const SolInstruction *,
  const SolAccountInfo *,
  int,
  const SolSignerSeeds *,
  int
);
#else
typedef uint64_t(*sol_invoke_signed_c_pointer_type)(
  const SolInstruction *,
  const SolAccountInfo *,
  int,
  const SolSignerSeeds *,
  int
);
static uint64_t sol_invoke_signed_c(
  const SolInstruction * arg1,
  const SolAccountInfo * arg2,
  int arg3,
  const SolSignerSeeds * arg4,
  int
 arg5) {
  sol_invoke_signed_c_pointer_type sol_invoke_signed_c_pointer = (sol_invoke_signed_c_pointer_type) 2720767109;
  return sol_invoke_signed_c_pointer(arg1, arg2, arg3, arg4, arg5);
}
#endif

/**
 * Invoke another program and sign for some of the keys
 *
 * @param instruction Instruction to process
 * @param account_infos Accounts used by instruction
 * @param account_infos_len Length of account_infos array
 * @param seeds Seed bytes used to sign program accounts
 * @param seeds_len Length of the seeds array
 */
static uint64_t sol_invoke_signed(
    const SolInstruction *instruction,
    const SolAccountInfo *account_infos,
    int account_infos_len,
    const SolSignerSeeds *signers_seeds,
    int signers_seeds_len
) {
  return sol_invoke_signed_c(
    instruction,
    account_infos,
    account_infos_len,
    signers_seeds,
    signers_seeds_len
  );
}
/**
 * Invoke another program
 *
 * @param instruction Instruction to process
 * @param account_infos Accounts used by instruction
 * @param account_infos_len Length of account_infos array
*/
static uint64_t sol_invoke(
    const SolInstruction *instruction,
    const SolAccountInfo *account_infos,
    int account_infos_len
) {
  const SolSignerSeeds signers_seeds[] = {{}};
  return sol_invoke_signed(
    instruction,
    account_infos,
    account_infos_len,
    signers_seeds,
    0
  );
}

#ifdef __cplusplus
}
#endif

/**@}*/
