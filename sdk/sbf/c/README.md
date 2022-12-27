## Development

### Quick start
To get started create a `makefile` containing:
```make
include path/to/sbf.mk
```
and `src/program.c` containing:
```c
#include <solana_sdk.h>

bool entrypoint(const uint8_t *input) {
  SolKeyedAccount ka[1];
  uint8_t *data;
  uint64_t data_len;

  if (!sol_deserialize(buf, ka, SOL_ARRAY_SIZE(ka), NULL, &data, &data_len)) {
    return false;
  }
  print_params(1, ka, data, data_len);
  return true;
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
