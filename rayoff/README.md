A Rust library that implements a fast threadpool.

Raw bench performance:

```
test bench_baseline ... bench:          90 ns/iter (+/- 5)
test bench_pool     ... bench:      10,489 ns/iter (+/- 2,053)
test bench_rayon    ... bench:      11,817 ns/iter (+/- 636)
```

sigverify performance:
```
running 3 tests
test bench_sigverify        ... bench:   3,973,128 ns/iter (+/- 306,527)
test bench_sigverify_rayoff ... bench:   3,697,677 ns/iter (+/- 738,464)
```
