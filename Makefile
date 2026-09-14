.PHONY: build demo site check check-types
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
	cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
check-types:
	@if command -v claude >/dev/null 2>&1; then \
		./scripts/check-plugin-types.sh; \
	else \
		echo "check-types: claude not on PATH, skipping"; \
	fi
