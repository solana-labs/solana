#pragma once
/**
 * @brief Solana types for SBF programs
 */

#ifdef __cplusplus
extern "C" {
#endif

/**
 * Pick up static_assert if C11 or greater
 *
 * Inlined here until <assert.h> is available
 */
#if (defined _ISOC11_SOURCE || (defined __STDC_VERSION__ && __STDC_VERSION__ >= 201112L)) && !defined (__cplusplus)
#undef static_assert
#define static_assert _Static_assert
#endif

/**
 * Numeric types
 */
#ifndef __LP64__
#error LP64 data model required
#endif

typedef signed char int8_t;
typedef unsigned char uint8_t;
typedef signed short int16_t;
typedef unsigned short uint16_t;
typedef signed int int32_t;
typedef unsigned int uint32_t;
typedef signed long int int64_t;
typedef unsigned long int uint64_t;
typedef int64_t ssize_t;
typedef uint64_t size_t;

#if defined (__cplusplus) || defined(static_assert)
static_assert(sizeof(int8_t) == 1);
static_assert(sizeof(uint8_t) == 1);
static_assert(sizeof(int16_t) == 2);
static_assert(sizeof(uint16_t) == 2);
static_assert(sizeof(int32_t) == 4);
static_assert(sizeof(uint32_t) == 4);
static_assert(sizeof(int64_t) == 8);
static_assert(sizeof(uint64_t) == 8);
#endif

/**
 * Minimum of signed integral types
 */
#define INT8_MIN   (-128)
#define INT16_MIN  (-32767-1)
#define INT32_MIN  (-2147483647-1)
#define INT64_MIN  (-9223372036854775807L-1)

/**
 * Maximum of signed integral types
 */
#define INT8_MAX   (127)
#define INT16_MAX  (32767)
#define INT32_MAX  (2147483647)
#define INT64_MAX  (9223372036854775807L)

/**
 * Maximum of unsigned integral types
 */
#define UINT8_MAX   (255)
#define UINT16_MAX  (65535)
#define UINT32_MAX  (4294967295U)
#define UINT64_MAX  (18446744073709551615UL)

/**
 * NULL
 */
#define NULL 0

/** Indicates the instruction was processed successfully */
#define SUCCESS 0

/**
 * Builtin program status values occupy the upper 32 bits of the program return
 * value.  Programs may define their own error values but they must be confined
 * to the lower 32 bits.
 */
#define TO_BUILTIN(error) ((uint64_t)(error) << 32)

/** Note: Not applicable to program written in C */
#define ERROR_CUSTOM_ZERO TO_BUILTIN(1)
/** The arguments provided to a program instruction where invalid */
#define ERROR_INVALID_ARGUMENT TO_BUILTIN(2)
/** An instruction's data contents was invalid */
#define ERROR_INVALID_INSTRUCTION_DATA TO_BUILTIN(3)
/** An account's data contents was invalid */
#define ERROR_INVALID_ACCOUNT_DATA TO_BUILTIN(4)
/** An account's data was too small */
#define ERROR_ACCOUNT_DATA_TOO_SMALL TO_BUILTIN(5)
/** An account's balance was too small to complete the instruction */
#define ERROR_INSUFFICIENT_FUNDS TO_BUILTIN(6)
/** The account did not have the expected program id */
#define ERROR_INCORRECT_PROGRAM_ID TO_BUILTIN(7)
/** A signature was required but not found */
#define ERROR_MISSING_REQUIRED_SIGNATURES TO_BUILTIN(8)
/** An initialize instruction was sent to an account that has already been initialized */
#define ERROR_ACCOUNT_ALREADY_INITIALIZED TO_BUILTIN(9)
/** An attempt to operate on an account that hasn't been initialized */
#define ERROR_UNINITIALIZED_ACCOUNT TO_BUILTIN(10)
/** The instruction expected additional account keys */
#define ERROR_NOT_ENOUGH_ACCOUNT_KEYS TO_BUILTIN(11)
/** Note: Not applicable to program written in C */
#define ERROR_ACCOUNT_BORROW_FAILED TO_BUILTIN(12)
/** The length of the seed is too long for address generation */
#define MAX_SEED_LENGTH_EXCEEDED TO_BUILTIN(13)
/** Provided seeds do not result in a valid address */
#define INVALID_SEEDS TO_BUILTIN(14)

/**
 * Boolean type
 */
#ifndef __cplusplus
#include <stdbool.h>
#endif

/**
 * Computes the number of elements in an array
 */
#define SOL_ARRAY_SIZE(a) (sizeof(a) / sizeof(a[0]))

/**
 * Byte array pointer and string
 */
typedef struct {
  const uint8_t *addr; /** bytes */
  uint64_t len; /** number of bytes*/
} SolBytes;

#ifdef __cplusplus
}
#endif

/**@}*/
