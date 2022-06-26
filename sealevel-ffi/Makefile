.SUFFIXES:

RUST_SOURCES:=$(wildcard ./src/*.rs)

sealevel.h: cbindgen.toml $(RUST_SOURCES)
	cbindgen -o sealevel.h
