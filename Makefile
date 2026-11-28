.PHONY: wasm build demo site check check-types check-contract check-facts
build:
	cargo build --release
site:
	./scripts/build-site.sh
	python3 scripts/build-guide.py
demo: build
	python3 scripts/demo.py
	./scripts/build-site.sh
	python3 scripts/build-guide.py
check:
	cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
check-types:
	@if command -v claude >/dev/null 2>&1; then \
		./scripts/check-plugin-types.sh; \
	else \
		echo "check-types: claude not on PATH, skipping"; \
	fi
check-contract:
	bash scripts/test-extract-dts.sh
	./scripts/check-contract.sh
check-facts:
	python3 scripts/check-harness-facts.py

# plugin/hooks/charts.wasm, the one build output under plugin/. Uses the
# isolated toolchain (rustup with wasm32-unknown-unknown, no PATH changes)
# when it exists, else the cargo on PATH (CI). charts.wasm.inputs records
# what it was built from; tests/wasm_fresh.rs fails when the sources move.
WASM_TOOLCHAIN ?= $(HOME)/.cctop-toolchain
WASM_BIN := $(WASM_TOOLCHAIN)/cargo/bin
WASM_ENV := $(if $(wildcard $(WASM_BIN)/cargo),RUSTUP_HOME=$(WASM_TOOLCHAIN)/rustup RUSTC=$(WASM_BIN)/rustc,)
WASM_CARGO := $(if $(wildcard $(WASM_BIN)/cargo),$(WASM_BIN)/cargo,cargo)
WASM_RUSTC := $(if $(wildcard $(WASM_BIN)/cargo),$(WASM_BIN)/rustc,rustc)
wasm:
	$(WASM_ENV) RUSTFLAGS="-C link-arg=--max-memory=67108864" $(WASM_CARGO) build --target wasm32-unknown-unknown --profile wasm -p cctop-charts
	cp target/wasm32-unknown-unknown/wasm/cctop_charts.wasm plugin/hooks/charts.wasm
	@if command -v wasm-opt >/dev/null 2>&1; then wasm-opt -Oz plugin/hooks/charts.wasm -o plugin/hooks/charts.wasm; fi
	printf 'sources %s\nrustc %s\n' "$$(python3 scripts/wasm-inputs.py)" "$$($(WASM_ENV) $(WASM_RUSTC) -V)" > plugin/hooks/charts.wasm.inputs
	@ls -l plugin/hooks/charts.wasm
