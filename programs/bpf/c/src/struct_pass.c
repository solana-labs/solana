#include <solana_sdk.h>

struct foo {const uint8_t *input;};
void foo(const uint8_t *input, struct foo foo) ;

extern bool entrypoint(const uint8_t *input) {
  struct foo f;
  f.input = input;
  foo(input, f);
  return true;
}

void foo(const uint8_t *input, struct foo foo) {
  sol_log_64(0, 0, 0, (uint64_t)input, (uint64_t)foo.input);
  sol_assert(input == foo.input);
}

