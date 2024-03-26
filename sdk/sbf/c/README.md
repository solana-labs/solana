## Development

### Quick start
To get started create a `makefile` containing:
```make
include path/to/sbf.mk
```
and `src/program.c` containing:
```c
#include <solana_sdk.h>

extern uint64_t entrypoint(const uint8_t *input) {
  SolAccountInfo ka[1];
  SolParameters params = (SolParameters) { .ka = ka };

  if (!sol_deserialize(input, &params, SOL_ARRAY_SIZE(ka))) {
    return ERROR_INVALID_ARGUMENT;
  }
  return SUCCESS;
}
```

Then run `make` to build `out/program.o`.
Run `make help` for more details.

### Unit tests
Built-in support for unit testing is provided by the
[Criterion](https://criterion.readthedocs.io/en/master/index.html) test framework.
To get started create the file `test/example.c` containing:
```c
#include <criterion/criterion.h>
#include "../src/program.c"

Test(test_suite_name, test_case_name) {
  cr_assert(true);
}
```
Then run `make test`.

### Limitations
* Programs must be fully contained within a single .c file
* No libc is available but `solana_sdk.h` provides a minimal set of primitives
