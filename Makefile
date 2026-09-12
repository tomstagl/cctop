.PHONY: build demo site check
build:
	cargo build --release
site: 
	./scripts/build-site.sh
demo: build
	python3 scripts/demo.py
	./scripts/build-site.sh
check:
	cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test
