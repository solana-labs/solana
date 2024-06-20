SBF_SDK_PATH := ../../sdk/sbf
SRC_DIR := c/src
OUT_DIR := target/sbf-solana-solana/release

test: rust all
	SBF_OUT_DIR=$(OUT_DIR) cargo test --features="sbf_rust,sbf_c" $(TEST_ARGS)

rust:
	cargo +solana build --release --target sbf-solana-solana --workspace

.PHONY: rust

include $(SBF_SDK_PATH)/c/sbf.mk
