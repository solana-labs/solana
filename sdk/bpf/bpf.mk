LOCAL_PATH := $(dir $(lastword $(MAKEFILE_LIST)))
INSTALL_SH := $(abspath $(LOCAL_PATH)/scripts/install.sh)

all:
.PHONY: help all clean

ifneq ($(V),1)
_@ :=@
endif

INC_DIRS ?=
SRC_DIR ?= ./src
TEST_DIR ?= ./test
OUT_DIR ?= ./out
OS := $(shell uname)

ifeq ($(DOCKER),1)
$(warning DOCKER=1 is experimential and may not work as advertised)
LLVM_DIR = $(LOCAL_PATH)llvm-docker/
LLVM_SYSTEM_INC_DIRS := /usr/local/lib/clang/8.0.0/include
else
LLVM_DIR = $(LOCAL_PATH)llvm-native/
LLVM_SYSTEM_INC_DIRS := $(LLVM_DIR)/lib/clang/8.0.0/include
endif

ifdef LLVM_DIR
CC := $(LLVM_DIR)/bin/clang
CXX := $(LLVM_DIR)/bin/clang++
LLD := $(LLVM_DIR)/bin/ld.lld
OBJ_DUMP := $(LLVM_DIR)/bin/llvm-objdump
endif

SYSTEM_INC_DIRS := \
  $(LOCAL_PATH)inc \
  $(LLVM_SYSTEM_INC_DIRS) \

C_FLAGS := \
  -Werror \
  -O2 \
  -fno-builtin \
  -std=c17 \
  $(addprefix -isystem,$(SYSTEM_INC_DIRS)) \
  $(addprefix -I,$(INC_DIRS))

CXX_FLAGS := \
  $(C_FLAGS) \
  -std=c++17 \

BPF_C_FLAGS := \
  $(C_FLAGS) \
  -target bpf \
  -fPIC \

BPF_CXX_FLAGS := \
  $(CXX_FLAGS) \
  -target bpf \
  -fPIC \
  -fomit-frame-pointer \
  -fno-exceptions \
  -fno-asynchronous-unwind-tables \
  -fno-unwind-tables \

BPF_LLD_FLAGS := \
  -z notext \
  -shared \
  --Bdynamic \
  $(LOCAL_PATH)bpf.ld \

OBJ_DUMP_FLAGS := \
  -color \
  -source \
  -disassemble \

TESTFRAMEWORK_RPATH := $(abspath $(LOCAL_PATH)criterion/lib)
TESTFRAMEWORK_FLAGS := \
  -DSOL_TEST \
  -isystem $(LOCAL_PATH)criterion/include \
  -L $(LOCAL_PATH)criterion/lib \
  -rpath $(TESTFRAMEWORK_RPATH) \
  -lcriterion \

# The "-rpath" in TESTFRAMEWORK_FLAGS doesn't work in macOS so rewrite the name
# post-link.
# TODO: Find a better way
MACOS_ADJUST_TEST_DYLIB := \
$(if $(filter $(OS),Darwin),\
 $(_@)install_name_tool -change libcriterion.3.dylib $(TESTFRAMEWORK_RPATH)/libcriterion.3.dylib, \
 : \
)

TEST_C_FLAGS := \
  $(C_FLAGS) \
  $(TESTFRAMEWORK_FLAGS) \

TEST_CXX_FLAGS := \
  $(CXX_FLAGS) \
  $(TESTFRAMEWORK_FLAGS) \

help:
	@echo ''
	@echo 'BPF Program makefile'
	@echo ''
	@echo 'This makefile will build BPF Programs from C or C++ source files into ELFs'
	@echo ''
	@echo 'Assumptions:'
	@echo '  - Programs are a single .c or .cc source file (may include headers)'
	@echo '  - Programs are located in the source directory: $(SRC_DIR)'
	@echo '  - Programs are named by their basename (eg. file name:foo.c/foo.cc -> program name:foo)'
	@echo '  - Tests are located in the test directory: $(TEST_DIR)'
	@echo '  - Tests are named by their basename (eg. file name:foo.c/foo.cc -> test name:test_foo)'
	@echo '  - Output files will be placed in the directory: $(OUT_DIR)'
	@echo ''
	@echo 'User settings'
	@echo '  - The following setting are overridable on the command line, default values shown:'
	@echo '    - Show commands while building: V=1'
	@echo '      V=$(V)'
	@echo '    - List of include directories:'
	@echo '      INC_DIRS=$(INC_DIRS)'
	@echo '    - List of system include directories:'
	@echo '      SYSTEM_INC_DIRS=$(SYSTEM_INC_DIRS)'
	@echo '    - Location of source files:'
	@echo '      SRC_DIR=$(SRC_DIR)'
	@echo '    - Location of test files:'
	@echo '      TEST_DIR=$(TEST_DIR)'
	@echo '    - Location to place output files:'
	@echo '      OUT_DIR=$(OUT_DIR)'
	@echo '    - Location of LLVM:'
	@echo '      LLVM_DIR=$(LLVM_DIR)'
	@echo ''
	@echo 'Usage:'
	@echo '  - make help - This help message'
	@echo '  - make all - Build all the programs'
	@echo '  - make test - Build and run all tests'
	@echo '  - make dump_<program name> - Dumps the contents of the program to stdout'
	@echo '  - make <program name> - Build a single program by name'
	@echo ''
	@echo 'Available programs:'
	$(foreach name, $(PROGRAM_NAMES), @echo '  - $(name)'$(\n))
	@echo 'Available tests:'
	$(foreach name, $(TEST_NAMES), @echo '  - $(name)'$(\n))
	@echo ''
	@echo 'Example:'
	@echo '  - Assuming a programed named foo (src/foo.c)'
	@echo '    - make foo'
	@echo '    - make dump_foo'
	@echo ''

.PHONY: $(INSTALL_SH)
$(INSTALL_SH):
	$(INSTALL_SH)

.PRECIOUS: $(OUT_DIR)/%.o
$(OUT_DIR)/%.o: $(SRC_DIR)/%.c $(INSTALL_SH)
	@echo "[cc] $@ ($<)"
	$(_@)mkdir -p $(OUT_DIR)
	$(_@)$(CC) $(BPF_C_FLAGS) -o $@ -c $< -MD -MF $(@:.o=.d)

$(OUT_DIR)/%.o: $(SRC_DIR)/%.cc $(INSTALL_SH)
	@echo "[cxx] $@ ($<)"
	$(_@)mkdir -p $(OUT_DIR)
	$(_@)$(CXX) $(BPF_CXX_FLAGS) -o $@ -c $< -MD -MF $(@:.o=.d)

.PRECIOUS: $(OUT_DIR)/%.so
$(OUT_DIR)/%.so: $(OUT_DIR)/%.o $(INSTALL_SH)
	@echo "[lld] $@ ($<)"
	$(_@)$(LLD) $(BPF_LLD_FLAGS) -o $@ $<

$(OUT_DIR)/test_%: $(TEST_DIR)/%.c $(INSTALL_SH)
	@echo "[test cc] $@ ($<)"
	$(_@)mkdir -p $(OUT_DIR)
	$(_@)$(CC) $(TEST_C_FLAGS) -o $@ $< -MD -MF $(@:=.d)
	$(_@)$(MACOS_ADJUST_TEST_DYLIB) $@

$(OUT_DIR)/test_%: $(TEST_DIR)/%.cc $(INSTALL_SH)
	@echo "[test cxx] $@ ($<)"
	$(_@)mkdir -p $(OUT_DIR)
	$(_@)$(CXX) $(TEST_CXX_FLAGS) -o $@ $< -MD -MF $(@:=.d)
	$(_@)$(MACOS_ADJUST_TEST_DYLIB) $@

-include $(wildcard $(OUT_DIR)/*.d)

PROGRAM_NAMES := $(notdir $(basename $(wildcard $(SRC_DIR)/*.c $(SRC_DIR)/*.cc)))
TEST_NAMES := $(addprefix test_,$(notdir $(basename $(wildcard $(TEST_DIR)/*.c))))

define \n


endef

all: $(PROGRAM_NAMES)

test: $(TEST_NAMES)
	$(foreach test, $(TEST_NAMES), $(OUT_DIR)/$(test)$(\n))

$(PROGRAM_NAMES): %: $(addprefix $(OUT_DIR)/, %.so) ;
$(TEST_NAMES): %: $(addprefix $(OUT_DIR)/, %) ;

dump_%: %
	$(_@)$(OBJ_DUMP) $(OBJ_DUMP_FLAGS) $(addprefix $(OUT_DIR)/, $(addsuffix .so, $<))

clean:
	rm -rf $(OUT_DIR)
