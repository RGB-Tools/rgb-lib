CC        = gcc
CFLAGS    = -g
RM        = rm -f
BUILDDIR = target/debug
LDFLAGS = -L$(BUILDDIR) -lrgblibcffi -static -lm -ldl -lpthread -ljson-c

REGTEST   = ../../tests/regtest.sh

default: all

all: example

rust-build:
	cargo build

build-example:
	$(CC) $(CFLAGS) -o example example.c $(LDFLAGS)

example:
	$(MAKE) rust-build
	$(MAKE) build-example

clean:
	cargo clean
	$(RM) example

format:
	clang-format -i --style="{BasedOnStyle: llvm, IndentWidth: 4}" *.c
	cargo fmt

regtest_start:
	$(REGTEST) prepare_bindings_examples_environment

regtest_stop:
	$(REGTEST) stop_services

.PHONY: all rust-build build-example example clean format regtest_start regtest_stop
