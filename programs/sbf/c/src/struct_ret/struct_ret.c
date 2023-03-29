#include <solana_sdk.h>

/**
 * Custom error for when struct doesn't add to 12
 */
#define INCORRECT_SUM 1

struct test_struct { uint64_t x; uint64_t y; uint64_t z;};

static struct test_struct __attribute__ ((noinline)) test_function(void) {
  struct test_struct s;
  s.x = 3;
  s.y = 4;
  s.z = 5;
  return s;
}

extern uint64_t entrypoint(const uint8_t* input) {
  struct test_struct s = test_function();
  sol_log("foobar");
  if (s.x + s.y + s.z == 12 ) {
    return SUCCESS;
  }
  return INCORRECT_SUM;
}
