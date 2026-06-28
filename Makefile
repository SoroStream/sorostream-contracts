WASM_TARGET  := wasm32v1-none
WASM_DIR     := target/$(WASM_TARGET)/release
WASM_FILE    := $(WASM_DIR)/sorostream_stream.wasm
WASM_OPT_OUT := $(WASM_DIR)/sorostream_stream.optimized.wasm
SIZE_LOG     := wasm-size.log

.PHONY: build build-size optimize clean check-size

build:
	cargo build --target $(WASM_TARGET) --release

build-size:
	cargo build --target $(WASM_TARGET) --profile release-size

optimize: build-size
	wasm-opt -Oz --strip-debug --strip-producers $(WASM_FILE) -o $(WASM_OPT_OUT)
	@echo "Optimized WASM size: $$(wc -c < $(WASM_OPT_OUT)) bytes"

check-size: optimize
	@SIZE=$$(wc -c < $(WASM_OPT_OUT)); \
	echo "$$SIZE" > $(SIZE_LOG); \
	echo "Current WASM binary size: $$SIZE bytes"

clean:
	cargo clean
