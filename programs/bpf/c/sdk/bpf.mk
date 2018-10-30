
all:
.PHONY: help all clean

ifneq ($(V),1)
_@ :=@
endif

INC_DIRS :=
SYSTEM_INC_DIRS := -isystem $(dir $(lastword $(MAKEFILE_LIST)))inc
SRC_DIR := ./src
OUT_DIR := ./out

OS=$(uname)
ifeq ($(OS),Darwin)
LLVM_DIR := $(brew --prefix llvm)
else
LLVM_DIR := /usr/local/opt/llvm
endif

CC := $(LLVM_DIR)/bin/clang
CC_FLAGS := \
  -Werror \
  -target bpf \
  -O2 \
  -emit-llvm \
  -fno-builtin \

LLC := $(LLVM_DIR)/bin/llc
LLC_FLAGS := \
  -march=bpf \
  -filetype=obj \
  -function-sections \

OBJ_DUMP := $(LLVM_DIR)/bin/llvm-objdump
OBJ_DUMP_FLAGS := \
  -color \
  -source \
  -disassemble \

help:
	@echo 'BPF Program makefile'
	@echo ''
	@echo 'This makefile will build BPF Programs from C source files into ELFs'
	@echo ''
	@echo 'Assumptions:'
	@echo '  - Programs are a single .c source file (may include headers)'
	@echo '  - Programs are located in the source directory: $(SRC_DIR)'
	@echo '  - Programs are named by their basename (eg. file name:foo.c -> program name:foo)'
	@echo '  - Output files will be placed in the directory: $(OUT_DIR)'
	@echo ''
	@echo 'User settings'
	@echo '  - The following setting are overridable on the command line, default values shown'
	@echo '    - Show commands:'
	@echo '      V=1'
	@echo '    - List of include dirs:'
	@echo '      INC_DIRS=$(INC_DIRS)'
	@echo '    - Location of source files:'
	@echo '      SRC_DIR=$(SRC_DIR)'
	@echo '    - Location to place output files:'
	@echo '      OUT_DIR=$(OUT_DIR)'
	@echo '    - Location of LLVM:'
	@echo '      LLVM_DIR=$(LLVM_DIR)'
	@echo ''
	@echo 'Usage:'
	@echo '  - make help - This help message'
	@echo '  - make all - Builds all the programs in the directory: $(SRC_DIR)'
	@echo '  - make clean - Cleans all programs'
	@echo '  - make dump_<program name> - Dumps the contents of the program to stdout'
	@echo '  - make <program name> - Build a single program by name'
	@echo ''
	@echo 'Available programs:'
	$(foreach name, $(PROGRAM_NAMES), @echo '  - $(name)'$(\n))
	@echo ''
	@echo 'Example:'
	@echo '  - Assuming a programed named foo (src/foo.c)'
	@echo '    - make foo'
	@echo '    - make dump_foo'

.PRECIOUS: $(OUT_DIR)/%.bc
$(OUT_DIR)/%.bc: $(SRC_DIR)/%.c
	@echo "[cc] $@ ($<)"
	$(_@)mkdir -p $(OUT_DIR)
	$(_@)$(CC) $(CC_FLAGS) $(SYSTEM_INC_DIRS) $(INC_DIRS) -o $@ -c $< -MD -MF $(@:.bc=.d)

.PRECIOUS: $(OUT_DIR)/%.o
$(OUT_DIR)/%.o: $(OUT_DIR)/%.bc
	@echo "[llc] $@ ($<)"
	$(_@)$(LLC) $(LLC_FLAGS) -o $@ $<

-include $(wildcard $(OUT_DIR)/*.d)

PROGRAM_NAMES := $(notdir $(basename $(wildcard src/*.c)))

define \n


endef

all: $(PROGRAM_NAMES)

%: $(addprefix $(OUT_DIR)/, %.o) ;

dump_%: %
	$(_@)$(OBJ_DUMP) $(OBJ_DUMP_FLAGS) $(addprefix $(OUT_DIR)/, $(addsuffix .o, $<))

clean:
	rm -rf $(OUT_DIR)
