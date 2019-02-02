LOCAL_PATH := $(dir $(lastword $(MAKEFILE_LIST)))
SDK_PATH := $(abspath $(LOCAL_PATH)/../../../../sdk/bpf)
INSTALL_SH := $(abspath $(SDK_PATH)/scripts/install.sh)

all:
.PHONY: help all install dump clean

ifneq ($(V),1)
_@ :=@
endif

TARGET_NAME := solana_bpf_rust_noop
SRC_DIR ?= ./src
OUT_DIR ?= ./out
INSTALL_DIR ?= ./out
CARGO_OUT_DIR ?=$(LOCAL_PATH)target/release

ifeq ($(DOCKER),1)
$(warning DOCKER=1 is experimential and may not work as advertised)
LLVM_DIR = $(SDK_PATH)/llvm-docker
LLVM_SYSTEM_INC_DIRS := /usr/local/lib/clang/8.0.0/include
else
LLVM_DIR = $(SDK_PATH)/llvm-native
LLVM_SYSTEM_INC_DIRS := $(LLVM_DIR)/lib/clang/8.0.0/include
endif

CARGO := cargo
ifdef LLVM_DIR
LLC := $(LLVM_DIR)/bin/llc
LLD := $(LLVM_DIR)/bin/ld.lld
OBJ_COPY := $(LLVM_DIR)/bin/llvm-objcopy
OBJ_DUMP := $(LLVM_DIR)/bin/llvm-objdump
endif

CARGO_FLAGS := \
  +nightly \
  -vv rustc \
  -vv \
  --release \
  -- \
  --emit=llvm-ir \
  -C panic=abort \

LLC_FLAGS := \
  -march=bpf \
  -filetype=obj \

LLD_FLAGS := \
  -z notext \
  -shared \
  --Bdynamic \
  $(LOCAL_PATH)bpf.ld \

OBJ_COPY_FLAGS := \
  --remove-section .eh_frame \

OBJ_DUMP_FLAGS := \
  -color \
  -source \
  -disassemble \

help:
	@echo ''
	@echo 'solana-bpf-rust-noop makefile'
	@echo ''
	@echo 'This makefile will build the solana-bpf-rust-noop crate into a BPF shared object'
	@echo ''
	@echo 'This makefile is not run as part of the Solana workspace.  Doing so'
	@echo 'would result in a cargo deadlock since this makefile also calls cargo with parameters'
	@echo 'required to build the BPF program from Rust.'  
	@echo ''
	@echo 'Note: Rust BPF programs are tested as part of the Solana integration tests when'
	@echo '      feature "bpf_rust" is enabled.  The solana-bpf-rust-noop crate must be built'
	@echo '      with this makefile first before bulding Solana.'
	@echo ''
	@echo '      Here is a sample command that will run this BPF program:'
	@echo ''
	@echo '      export RUST_LOG=solana_bpf_loader=info; cargo test --features="bpf_rust" -- --nocapture test_program_bpf_rust'
	@echo ''
	@echo 'User settings'
	@echo '  - The following setting are overridable on the command line, default values shown:'
	@echo '    - Show commands while building: V=1'
	@echo '      V=$(V)'
	@echo '    - Location to place output files:'
	@echo '      OUT_DIR=$(OUT_DIR)'
	@echo '    - Location to install the final shared object:'
	@echo '      INSTALL_DIR=$(INSTALL_DIR)'
	@echo '    - Location of LLVM:'
	@echo '      LLVM_DIR=$(LLVM_DIR)'
	@echo ''
	@echo 'Usage:'
	@echo '  - make help - This help message'
	@echo '  - make all  - Build $(OUT_DIR)/$(TARGET_NAME).so'
	@echo '  - make dump - Dumps the contents of $(OUT_DIR)/$(TARGET_NAME).so to stdout, requires greadelf and rustfilt'
	@echo ''

.PHONY: $(INSTALL_SH)
$(INSTALL_SH):
	$(_@)$(INSTALL_SH)

.PRECIOUS: $(OUT_DIR)/%.ll
$(OUT_DIR)/%.ll: $(SRC_DIR)/*
	@echo "[cargo] $@ ($<)"
	$(_@)mkdir -p $(OUT_DIR)
	$(_@)rm -f $(CARGO_OUT_DIR)/deps/$(TARGET_NAME)-*.ll
	$(_@)$(CARGO) $(CARGO_FLAGS)
	$(_@)cp $(CARGO_OUT_DIR)/deps/$(TARGET_NAME)-*.ll $(OUT_DIR)/$(TARGET_NAME).ll

.PRECIOUS: $(OUT_DIR)/%.o
$(OUT_DIR)/%.o: $(OUT_DIR)/%.ll $(INSTALL_SH)
	@echo "[llc] $@ ($<)"
	$(_@)$(LLC) $(LLC_FLAGS) -o $@ $<
	$(_@)$(OBJ_COPY) $(OBJ_COPY_FLAGS) $@

.PRECIOUS: $(OUT_DIR)/%.so
$(OUT_DIR)/%.so: $(OUT_DIR)/%.o $(INSTALL_SH)
	@echo "[lld] $@ ($<)"
	$(_@)$(LLD) $(LLD_FLAGS) -o $@ $<

-include $(wildcard $(OUT_DIR)/$(TARGET_NAME).d)

define \n


endef

all: $(OUT_DIR)/$(TARGET_NAME).so

# Warning: Do not build as part of install (e.g. install must not depend
# on $(TARGET_NAME).so) doing so will deadlock cargo due to recrusive
# calls to cargo
install:
	$(_@)mkdir -p $(INSTALL_DIR)
	$(_@)cp $(OUT_DIR)/$(TARGET_NAME).so $(INSTALL_DIR)

dump: $(OUT_DIR)/$(TARGET_NAME).so
	$(_@)greadelf -aW $(OUT_DIR)/$(TARGET_NAME).so | rustfilt
	$(_@)$(OBJ_DUMP) -disassemble -source $(OUT_DIR)/$(TARGET_NAME).so | rustfilt

test:
	cargo test -- --test-threads 1

clean:
	$(_@)rm -rf $(OUT_DIR)
	cargo clean
