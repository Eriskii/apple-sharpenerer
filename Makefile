CARGO   ?= cargo
OUT     := target/dist
NIGHTLY := -Zbuild-std=core,alloc

# macOS 26 only; also makes the linker emit chained fixups, which is what the injector reads.
export MACOSX_DEPLOYMENT_TARGET := 26.0

.PHONY: all payload thread cli test clean devhost

all: payload thread cli

# One arm64 build, mapped into every app by the injector (arm64e apps included).
payload:
	$(CARGO) build --release -p payload $(NIGHTLY) --target aarch64-apple-darwin
	mkdir -p $(OUT)
	cp target/aarch64-apple-darwin/release/libsharpenerer.dylib $(OUT)/

# Must be arm64e: it signs the payload thread's start address for arm64e apps.
thread:
	$(CARGO) build --release -p thread $(NIGHTLY) --target arm64e-apple-darwin
	mkdir -p $(OUT)
	cp target/arm64e-apple-darwin/release/sharpenerer-thread $(OUT)/

cli:
	$(CARGO) build --release -p sharpenerer
	mkdir -p $(OUT)
	cp target/release/sharpenerer $(OUT)/

test:
	$(CARGO) test

clean:
	$(CARGO) clean

# Loads the payload into a test app and captures windows (development only; no root needed).
devhost: payload
	clang -fobjc-arc -framework AppKit tools/devhost.m -o $(OUT)/devhost
