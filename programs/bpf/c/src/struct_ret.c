#include <solana_sdk.h>

struct foo {const uint8_t *input;};
struct foo bar(const uint8_t *input);

extern bool entrypoint(const uint8_t *input) {
  struct foo foo = bar(input);
  sol_log_64(0, 0, 0, (uint64_t)input, (uint64_t)foo.input);
  sol_assert(input == foo.input);
  return true;
}

struct foo bar(const uint8_t *input) {
  struct foo foo;
  foo.input = input;
  return foo;
}

