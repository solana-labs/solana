_@ :=@
OUT_DIR := out
LLVM_DIR := /usr/local/opt/llvm/bin#_

CC = $(LLVM_DIR)/clang
CC_FLAGS = \
	-Werror \
	-target \
	bpf -O2 \
	-emit-llvm \
	-fno-builtin

LD = $(LLVM_DIR)/llc
LD_FLAGS = \
	-march=bpf \
	-filetype=obj \
	-function-sections

OBJ_DUMP = $(LLVM_DIR)/llvm-objdump
OBJ_DUMP_FLAGS = \
	-color \
	-source \
	-disassemble

SRC_DIR = ./src
INC_DIRS = -I.
PROGRAM_NAMES := $(notdir $(basename $(wildcard src/*.c)))

.PRECIOUS: $(OUT_DIR)/%.bc
$(OUT_DIR)/%.bc: $(SRC_DIR)/%.c
	$(_@)mkdir -p $(OUT_DIR)
	$(_@)$(CC) $(CC_FLAGS) $(INC_DIRS) -o $@ -c $<  -MMD -MF $(@:.bc=.d)

.PRECIOUS: $(OUT_DIR)/%.o
$(OUT_DIR)/%.o: $(OUT_DIR)/%.bc
	$(_@)$(LD) $(LD_FLAGS) -o $@ $<

-include $(wildcard $(OUT_DIR)/*.d)

define \n


endef

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
	@echo '    - OUT_DIR = $(OUT_DIR)'
	@echo '    - LLVM_DIR = $(LLVM_DIR)'
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

%: $(addprefix $(OUT_DIR)/, %.o)
	@echo $@ up to date

.PHONY: help all dump clean

all: $(PROGRAM_NAMES)

dump_%: %
	$(_@)$(OBJ_DUMP) $(OBJ_DUMP_FLAGS) $(addprefix $(OUT_DIR)/, $(addsuffix .o, $<))

clean:
	$(_@)rm -rf $(OUT_DIR)

debug:
	@echo $(PROGRAM_NAMES)
	@echo $(TARGETS)
