#include <criterion/criterion.h>
#include "bench_alu.c"

Test(bench_alu, sanity) {
  uint64_t input[] = {500, 0};

  cr_assert(entrypoint((uint8_t *) input));

  cr_assert_eq(input[0], 500);
  cr_assert_eq(input[1], 5);
}
