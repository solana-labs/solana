# Notes:

#  TODO needs more investigation into what r10 is used for
#    -O2 adds changes the generated code from:
#      main:
#       0:	b7 01 00 00 00 00 00 00 	r1 = 0
#       1:	63 1a fc ff 00 00 00 00 	*(u32 *)(r10 - 4) = r1
#       2:	bf 10 00 00 00 00 00 00 	r0 = r1
#       3:	95 00 00 00 00 00 00 00 	exit
#    To:
#      main:
#       0:	b7 00 00 00 00 00 00 00 	r0 = 0
#       1:	95 00 00 00 00 00 00 00 	exit

# - building bpf module that includes stdint.h with clang only (no w/ llc) results in an error
#  $(TOOLS_DIR)/clang -nostdlib -O2 -fpie -target bpf -o bpf.o -c bpf.c
#  => n file included from /usr/include/sys/_types/_intptr_t.h:30:
#  => /usr/include/machine/types.h:37:2: error: architecture not supported
#  => error architecture not supported

TOOLS_DIR = /usr/local/opt/llvm/bin

test_x86.o: test.c
	$(TOOLS_DIR)/clang -O2 -fpie -target x86_64 -o $@ -c $<

test_x86.so: test_x86.o
	$(TOOLS_DIR)/ld.lld --shared -o $@ $<

test_x86_dylib.o: test.c
	$(TOOLS_DIR)/clang -O2 -fpie -target x86_64-apple-darwin13.0.0 -o $@ -c $<

test_x86.dylib: test_x86_dylib.o
	/usr/local/opt/llvm/bin/ld64.lld -dylib -lc -arch x86_64 -o $@ $<

# TODO does not work if pulling in stdlib, claims unsupported architecture
bpf_clang.o: bpf.c
	$(TOOLS_DIR)/clang -nostdlib -O2 -fpie -target bpf -o bpf.o -c bpf.c

bpf.o: bpf.c
	$(TOOLS_DIR)/clang -O2 -emit-llvm -c $< -o - | $(TOOLS_DIR)/llc -march=bpf -filetype=obj -o $@

bpf_rust.o: bpf_rust.rs
	rustc +nightly -C opt-level=2 -C panic=abort --emit llvm-bc $< 
	$(TOOLS_DIR)/llc -march=bpf -filetype=obj -function-sections -o $@ bpf_rust.bc

dumpall:
	$(TOOLS_DIR)/llvm-objdump -color -source -disassemble *.o

cleanall:
	rm -f *.o
	rm -f *.so
	rm -f *.dylib
	rm -f *.ll
	rm -rf *.bc

all:  bpf_clang.o bpf.o

