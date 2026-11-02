CARGO   ?= cargo
SLICES  := aarch64-apple-darwin arm64e-apple-darwin
OUT     := target/universal

.PHONY: all payload cli test clean

all: payload cli

# One dylib with an arm64 slice (third-party apps) and an arm64e slice (Apple's apps).
payload:
	for t in $(SLICES); do \
		$(CARGO) build --release -p payload -Zbuild-std=core,alloc --target $$t || exit 1; \
	done
	mkdir -p $(OUT)
	lipo -create $(foreach t,$(SLICES),target/$(t)/release/libsharpenerer.dylib) -output $(OUT)/libsharpenerer.dylib

cli:
	$(CARGO) build --release -p sharpenerer
	mkdir -p $(OUT)
	cp target/release/sharpenerer $(OUT)/sharpenerer

test:
	$(CARGO) test

clean:
	$(CARGO) clean
