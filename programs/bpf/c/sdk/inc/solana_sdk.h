#pragma once
/**
 * @brief Solana C-based BPF program utility functions and types
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
 * NULL
 */
#define NULL 0

#ifndef __cplusplus
/**
 * Boolean type
 */
typedef enum { false = 0, true } bool;
#endif

/**
 * Helper function that prints a string to stdout
 */
extern void sol_log(const char*);

/**
 * Helper function that prints a 64 bit values represented in hexadecimal
 * to stdout
 */
extern void sol_log_64(uint64_t, uint64_t, uint64_t, uint64_t, uint64_t);

/**
 * Prefix for all BPF functions
 *
 * This prefix should be used for functions in order to facilitate
 * interoperability with BPF representation
 */
#define SOL_FN_PREFIX __attribute__((always_inline)) static

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
 * Compares two public keys
 *
 * @param one First public key
 * @param two Second public key
 * @return true if the same
 */
SOL_FN_PREFIX bool SolPubkey_same(const SolPubkey *one, const SolPubkey *two) {
  for (int i = 0; i < sizeof(*one); i++) {
    if (one->x[i] != two->x[i]) {
      return false;
    }
  }
  return true;
}

/**
 * Keyed Accounts
 */
typedef struct {
  SolPubkey *key;        /** Public Key of the account owner */
  int64_t *tokens;       /** Numer of tokens owned by this account */
  uint64_t userdata_len; /** Length of userdata in bytes */
  uint8_t *userdata;     /** On-chain data owned by this account */
  SolPubkey *program_id; /** Program that owns this account */
} SolKeyedAccounts;

/**
 * Copies memory
 */
SOL_FN_PREFIX void sol_memcpy(void *dst, const void *src, int len) {
  for (int i = 0; i < len; i++) {
    *((uint8_t *)dst + i) = *((const uint8_t *)src + i);
  }
}

/**
 * Compares memory
 */
SOL_FN_PREFIX int sol_memcmp(const void *s1, const void *s2, int n) {
  for (int i = 0; i < n; i++) {
    uint8_t diff = *((uint8_t *)s1 + i) - *((const uint8_t *)s2 + i);
    if (diff) {
      return diff;
    }
  }
  return 0;
}

/**
 * Fill a byte string with a byte value
 */
SOL_FN_PREFIX void *sol_memset(void *b, int c, size_t len) {
  uint8_t *a = (uint8_t *) b;
  while (len > 0) {
    *a = c;
    a++;
    len--;
  }
}

/**
 * Find length of string
 */
SOL_FN_PREFIX size_t sol_strlen(const char *s) {
  size_t len = 0;
  while (*s) {
    len++;
    s++;
  }
  return len;
}

/**
 * Computes the number of elements in an array
 */
#define SOL_ARRAY_SIZE(a) (sizeof(a) / sizeof(a[0]))

/**
 * Panics
 *
 * Prints the line number where the panic occurred and then causes
 * the BPF VM to immediately halt execution. No accounts' userdata are updated
 */
#define sol_panic() _sol_panic(__LINE__)
SOL_FN_PREFIX void _sol_panic(uint64_t line) {
  sol_log_64(0xFF, 0xFF, 0xFF, 0xFF, line);
  uint8_t *pv = (uint8_t *)1;
  *pv = 1;
}

/**
 * Asserts
 */
#define sol_assert(expr)  \
  if (!(expr)) {          \
    _sol_panic(__LINE__); \
  }

/**
 * Information about the state of the cluster immediately before the program
 * started executing the current instruction
 */
typedef struct {
  uint64_t tick_height; /** Current ledger tick */
} SolClusterInfo;

/**
 * De-serializes the input parameters into usable types
 *
 * Use this function to deserialize the buffer passed to the program entrypoint
 * into usable types.  This function does not perform copy deserialization,
 * instead it populates the pointers and lengths in SolKeyedAccounts and data so
 * that any modification to tokens or account data take place on the original
 * buffer.  Doing so also eliminates the need to serialize back into the buffer
 * at program end.
 *
 * @param input Source buffer containing serialized input parameters
 * @param ka Pointer to an array of SolKeyedAccounts to deserialize into
 * @param ka_len Number of SolKeyedAccounts entries in `ka`
 * @param ka_len_out If NULL, fill exactly `ka_len` accounts or fail.
 *                   If not NULL, fill up to `ka_len` accounts and return the
 *                   number of filled accounts in `ka_len_out`.
 * @param data On return, a pointer to the instruction data
 * @param data_len On return, the length in bytes of the instruction data
 * @param cluster_info If not NULL, fill cluster_info
 * @return Boolean true if successful
 */
SOL_FN_PREFIX bool sol_deserialize(
  const uint8_t *input,
  SolKeyedAccounts *ka,
  uint64_t ka_len,
  uint64_t *ka_len_out,
  const uint8_t **data,
  uint64_t *data_len,
  SolClusterInfo *cluster_info
) {


  if (ka_len_out == NULL) {
    if (ka_len != *(uint64_t *) input) {
      return false;
    }
    ka_len = *(uint64_t *) input;
  } else {
    if (ka_len > *(uint64_t *) input) {
      ka_len = *(uint64_t *) input;
    }
    *ka_len_out = ka_len;
  }

  input += sizeof(uint64_t);
  for (int i = 0; i < ka_len; i++) {
    // key
    ka[i].key = (SolPubkey *) input;
    input += sizeof(SolPubkey);

    // tokens
    ka[i].tokens = (int64_t *) input;
    input += sizeof(int64_t);

    // account userdata
    ka[i].userdata_len = *(uint64_t *) input;
    input += sizeof(uint64_t);
    ka[i].userdata = (uint8_t *) input;
    input += ka[i].userdata_len;

    // program_id
    ka[i].program_id = (SolPubkey *) input;
    input += sizeof(SolPubkey);
  }

  // input data
  *data_len = *(uint64_t *) input;
  input += sizeof(uint64_t);
  *data = input;
  input += *data_len;

  if (cluster_info != NULL) {
    cluster_info->tick_height = *(uint64_t *) input;
  }
  return true;
}

/**
 * Debugging utilities
 * @{
 */

/**
 * Prints the hexadecimal representation of a public key
 *
 * @param key The public key to print
 */
SOL_FN_PREFIX void sol_log_key(const SolPubkey *key) {
  for (int j = 0; j < sizeof(*key); j++) {
    sol_log_64(0, 0, 0, j, key->x[j]);
  }
}

/**
 * Prints the hexadecimal representation of an array
 *
 * @param array The array to print
 */
SOL_FN_PREFIX void sol_log_array(const uint8_t *array, int len) {
  for (int j = 0; j < len; j++) {
    sol_log_64(0, 0, 0, j, array[j]);
  }
}

/**
 * Prints the hexadecimal representation of the program's input parameters
 *
 * @param ka A pointer to an array of SolKeyedAccounts to print
 * @param ka_len Number of SolKeyedAccounts to print
 * @param data A pointer to the instruction data to print
 * @param data_len The length in bytes of the instruction data
 */
SOL_FN_PREFIX void sol_log_params(
  const SolKeyedAccounts *ka,
  uint64_t ka_len,
  const uint8_t *data,
  uint64_t data_len
) {
  sol_log_64(0, 0, 0, 0, ka_len);
  for (int i = 0; i < ka_len; i++) {
    sol_log_key(ka[i].key);
    sol_log_64(0, 0, 0, 0, *ka[i].tokens);
    sol_log_array(ka[i].userdata, ka[i].userdata_len);
    sol_log_key(ka[i].program_id);
  }
  sol_log_array(data, data_len);
}

/**@}*/

/**
 * Program instruction entrypoint
 *
 * @param input Buffer of serialized input parameters.  Use sol_deserialize() to decode
 * @return true if the instruction executed successfully
 */
extern bool entrypoint(const uint8_t *input);

#ifdef __cplusplus
}
#endif

/**@}*/
