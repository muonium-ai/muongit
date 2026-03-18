# muongit-swift: Native Swift port of libgit2
# Requires: Swift 5.9+

SWIFT       := swift
SWIFTC      := swiftc
PACKAGE     := muongit-swift
BUILD_DIR   := .build
SRC_DIR     := src
TESTS_DIR   := tests

# libgit2 reference source for parity checks
LIBGIT2_REF := ../vendor/libgit2

.PHONY: all build test clean fmt lint check-parity

all: build

build:
	$(SWIFT) build

release:
	$(SWIFT) build -c release

test:
	$(SWIFT) test

clean:
	$(SWIFT) package clean
	rm -rf $(BUILD_DIR)

fmt:
	swift-format format --in-place --recursive $(SRC_DIR)

lint:
	swift-format lint --recursive $(SRC_DIR)

check-parity:
	@echo "Checking API parity with libgit2..."
	@echo "Reference headers: $(LIBGIT2_REF)/include/git2/"
	@ls $(LIBGIT2_REF)/include/git2/*.h | wc -l | xargs -I{} echo "  libgit2 public APIs: {} headers"
	@ls $(SRC_DIR)/*.swift 2>/dev/null | wc -l | xargs -I{} echo "  Swift modules: {} files"
