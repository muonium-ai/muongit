# muongit-rust: Native Rust port of libgit2
# Requires: Rust 1.75+ (2021 edition)

CARGO       := cargo
PACKAGE     := muongit-rust
BUILD_DIR   := target
SRC_DIR     := src

# libgit2 reference source for parity checks
LIBGIT2_REF := ../vendor/libgit2

.PHONY: all build test clean fmt lint check-parity

all: build

build:
	$(CARGO) build

release:
	$(CARGO) build --release

test:
	$(CARGO) test

clean:
	$(CARGO) clean

fmt:
	$(CARGO) fmt

lint:
	$(CARGO) clippy -- -D warnings

check-parity:
	@echo "Checking API parity with libgit2..."
	@echo "Reference headers: $(LIBGIT2_REF)/include/git2/"
	@ls $(LIBGIT2_REF)/include/git2/*.h | wc -l | xargs -I{} echo "  libgit2 public APIs: {} headers"
	@find $(SRC_DIR) -name "*.rs" | wc -l | xargs -I{} echo "  Rust modules: {} files"
