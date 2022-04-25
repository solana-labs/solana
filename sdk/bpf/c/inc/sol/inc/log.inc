#pragma once
/**
 * @brief Solana logging utilities
 */

#include <sol/types.h>
#include <sol/string.h>
#include <sol/entrypoint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Prints a string to stdout
 */
@SYSCALL void sol_log_(const char *, uint64_t);
#define sol_log(message) sol_log_(message, sol_strlen(message))

/**
 * Prints a 64 bit values represented in hexadecimal to stdout
 */
@SYSCALL void sol_log_64_(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t);
#define sol_log_64 sol_log_64_

/**
 * Prints the current compute unit consumption to stdout
 */
@SYSCALL void sol_log_compute_units_();
#define sol_log_compute_units() sol_log_compute_units_()

/**
 * Prints the hexadecimal representation of an array
 *
 * @param array The array to print
 */
static void sol_log_array(const uint8_t *array, int len) {
  for (int j = 0; j < len; j++) {
    sol_log_64(0, 0, 0, j, array[j]);
  }
}

/**
 * Print the base64 representation of some arrays.
 */
@SYSCALL void sol_log_data(SolBytes *, uint64_t);

/**
 * Prints the program's input parameters
 *
 * @param params Pointer to a SolParameters structure
 */
static void sol_log_params(const SolParameters *params) {
  sol_log("- Program identifier:");
  sol_log_pubkey(params->program_id);

  sol_log("- Number of KeyedAccounts");
  sol_log_64(0, 0, 0, 0, params->ka_num);
  for (int i = 0; i < params->ka_num; i++) {
    sol_log("  - Is signer");
    sol_log_64(0, 0, 0, 0, params->ka[i].is_signer);
    sol_log("  - Is writable");
    sol_log_64(0, 0, 0, 0, params->ka[i].is_writable);
    sol_log("  - Key");
    sol_log_pubkey(params->ka[i].key);
    sol_log("  - Lamports");
    sol_log_64(0, 0, 0, 0, *params->ka[i].lamports);
    sol_log("  - data");
    sol_log_array(params->ka[i].data, params->ka[i].data_len);
    sol_log("  - Owner");
    sol_log_pubkey(params->ka[i].owner);
    sol_log("  - Executable");
    sol_log_64(0, 0, 0, 0, params->ka[i].executable);
    sol_log("  - Rent Epoch");
    sol_log_64(0, 0, 0, 0, params->ka[i].rent_epoch);
  }
  sol_log("- Instruction data\0");
  sol_log_array(params->data, params->data_len);
}

#ifdef SOL_TEST
/**
 * Stub functions when building tests
 */
#include <stdio.h>

void sol_log_(const char *s, uint64_t len) {
  printf("Program log: %s\n", s);
}
void sol_log_64(uint64_t arg1, uint64_t arg2, uint64_t arg3, uint64_t arg4, uint64_t arg5) {
  printf("Program log: %llu, %llu, %llu, %llu, %llu\n", arg1, arg2, arg3, arg4, arg5);
}

void sol_log_compute_units_() {
  printf("Program consumption: __ units remaining\n");
}
#endif

#ifdef __cplusplus
}
#endif

/**@}*/
