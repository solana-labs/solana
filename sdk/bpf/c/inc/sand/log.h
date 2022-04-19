#pragma once
/**
 * @brief Solana logging utilities
 */

#include <sand/types.h>
#include <sand/string.h>
#include <sand/entrypoint.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Prints a string to stdout
 */
void sand_log_(const char *, uint64_t);
#define sand_log(message) sand_log_(message, sand_strlen(message))

/**
 * Prints a 64 bit values represented in hexadecimal to stdout
 */
void sand_log_64_(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t);
#define sand_log_64 sand_log_64_

/**
 * Prints the current compute unit consumption to stdout
 */
void sand_log_compute_units_();
#define sand_log_compute_units() sand_log_compute_units_()

/**
 * Prints the hexadecimal representation of an array
 *
 * @param array The array to print
 */
static void sand_log_array(const uint8_t *array, int len) {
  for (int j = 0; j < len; j++) {
    sand_log_64(0, 0, 0, j, array[j]);
  }
}

/**
 * Print the base64 representation of some arrays.
 */
void sand_log_data(SandBytes *fields, uint64_t fields_len);

/**
 * Prints the program's input parameters
 *
 * @param params Pointer to a SandParameters structure
 */
static void sand_log_params(const SandParameters *params) {
  sand_log("- Program identifier:");
  sand_log_pubkey(params->program_id);

  sand_log("- Number of KeyedAccounts");
  sand_log_64(0, 0, 0, 0, params->ka_num);
  for (int i = 0; i < params->ka_num; i++) {
    sand_log("  - Is signer");
    sand_log_64(0, 0, 0, 0, params->ka[i].is_signer);
    sand_log("  - Is writable");
    sand_log_64(0, 0, 0, 0, params->ka[i].is_writable);
    sand_log("  - Key");
    sand_log_pubkey(params->ka[i].key);
    sand_log("  - Lamports");
    sand_log_64(0, 0, 0, 0, *params->ka[i].lamports);
    sand_log("  - data");
    sand_log_array(params->ka[i].data, params->ka[i].data_len);
    sand_log("  - Owner");
    sand_log_pubkey(params->ka[i].owner);
    sand_log("  - Executable");
    sand_log_64(0, 0, 0, 0, params->ka[i].executable);
    sand_log("  - Rent Epoch");
    sand_log_64(0, 0, 0, 0, params->ka[i].rent_epoch);
  }
  sand_log("- Instruction data\0");
  sand_log_array(params->data, params->data_len);
}

#ifdef SAND_TEST
/**
 * Stub functions when building tests
 */
#include <stdio.h>

void sand_log_(const char *s, uint64_t len) {
  printf("Program log: %s\n", s);
}
void sand_log_64(uint64_t arg1, uint64_t arg2, uint64_t arg3, uint64_t arg4, uint64_t arg5) {
  printf("Program log: %llu, %llu, %llu, %llu, %llu\n", arg1, arg2, arg3, arg4, arg5);
}

void sand_log_compute_units_() {
  printf("Program consumption: __ units remaining\n");
}
#endif

#ifdef __cplusplus
}
#endif

/**@}*/
