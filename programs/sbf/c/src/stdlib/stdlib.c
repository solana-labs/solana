/**
 * @brief Example C based SBF program that uses C standard
 * library functions.  The test fails if the C standard
 * library is not available.
 *
 * Only re-entrant versions of standard C library functions are
 * available.  We have to use the _reent structure and pass a pointer
 * to it as a parameter to a re-entrant function.  In this test
 * _atoi_r is a re-entrant version of the standard atoi function.
 */
#include <sol/assert.h>
#include <stdlib.h>

extern uint64_t entrypoint(const uint8_t *input) {
  struct _reent reent;
  int value = _atoi_r(&reent, "137");
  sol_assert(value == 137);

  return SUCCESS;
}
