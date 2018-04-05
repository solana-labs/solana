export RUST_LOG=solana::gpu=TRACE
#export RUST_BACKTRACE=1

all: htest

htest:wfmt
	#cargo test accountant_skel::tests::test_layout -- --nocapture 2>&1 | head -n 30
	#cargo test accountant_skel::tests::test_layout -- --nocapture
	cargo test accountant_stub -- --nocapture 2>&1 | head -n 30

ci: test bench release clippy ipv6

build:
	cargo build 2>&1 | head -n 30

loop:
	while true; do fswatch -1 -r src; make; done 

test:
	cargo test

clippy:
	 cargo +nightly clippy --features="unstable"

cov:
	docker run -it --rm --security-opt seccomp=unconfined --volume "$$PWD:/volume" elmtai/docker-rust-kcov

wfmt:
	cargo fmt -- --write-mode=overwrite

release:
	cargo build --all-targets --release

node:
	cat genesis.log | cargo run --bin silk-testnode > transactions0.log

bench:
	cargo +nightly bench --features="unstable" -- --nocapture

ipv6:
	cargo test ipv6 --features="ipv6" -- --nocapture

lib:libcuda_verify_ed25519.a
libcuda_verify_ed25519.a:dummy.c
	cc -o dummy.o -c dummy.c
	ar -cvq libcuda_verify_ed25519.a dummy.o
