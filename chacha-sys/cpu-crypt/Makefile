V:=debug

LIB:=cpu-crypt

CFLAGS_common:=-Wall -Werror -pedantic -fPIC
CFLAGS_release:=-march=native -O3 $(CFLAGS_common)
CFLAGS_debug:=-g $(CFLAGS_common)
CFLAGS:=$(CFLAGS_$V)

all: $V/lib$(LIB).a

$V/chacha20_core.o: chacha20_core.c chacha.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -c $< -o $@

$V/chacha_cbc.o: chacha_cbc.c chacha.h
	@mkdir -p $(@D)
	$(CC) $(CFLAGS) -c $< -o $@

$V/lib$(LIB).a: $V/chacha20_core.o $V/chacha_cbc.o
	$(AR) rcs $@ $^

.PHONY:clean
clean:
	rm -rf $V
