# muongit-kotlin: Native Kotlin port of libgit2
# Requires: Kotlin 2.0+, Gradle 8.x

GRADLE      := ./gradlew
PACKAGE     := muongit-kotlin
BUILD_DIR   := build
SRC_DIR     := src

# libgit2 reference source for parity checks
LIBGIT2_REF := ../vendor/libgit2

.PHONY: all build test clean fmt lint check-parity

all: build

build:
	$(GRADLE) build

release:
	$(GRADLE) build -Prelease=true

test:
	$(GRADLE) test

clean:
	$(GRADLE) clean

fmt:
	$(GRADLE) ktlintFormat

lint:
	$(GRADLE) ktlintCheck

check-parity:
	@echo "Checking API parity with libgit2..."
	@echo "Reference headers: $(LIBGIT2_REF)/include/git2/"
	@ls $(LIBGIT2_REF)/include/git2/*.h | wc -l | xargs -I{} echo "  libgit2 public APIs: {} headers"
	@find $(SRC_DIR) -name "*.kt" | wc -l | xargs -I{} echo "  Kotlin sources: {} files"
