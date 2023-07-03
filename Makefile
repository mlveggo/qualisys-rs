# all: all tasks required for a complete build
.PHONY: all
all: \
	commitlint \
	fmt-check \
	clippy \
	test \
	bench \
	build \
	git-verify-nodiff \

.PHONY: codequality
codequality: \
	commitlint \
	fmt-check \
	clippy \

include tools/commitlint/rules.mk
include tools/git-verify-nodiff/rules.mk

# GIT_BUILD_REV will be embeded in the plugin as build revision at buildtime
GIT_BUILD_REV ?= $(shell git describe --always --dirty)

.PHONY: clean
clean:
	rm -fR target

.PHONY: build
build:
	cargo build --all --all-targets

.PHONY: clippy
clippy:
	cargo clippy --all -- -D warnings

.PHONY: fmt-check
fmt-check:
	cargo fmt --all -- --check

.PHONY: test
test:
	RUST_BACKTRACE=1 cargo test --all

.PHONY: bench
bench:
	RUST_BACKTRACE=1 cargo bench
