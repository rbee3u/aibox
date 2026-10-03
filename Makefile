ARGS ?=

.PHONY: help deps generate style format web build test lint console

help:
	@printf "%s\n" \
		"Usage: make [target] [VARIABLE=value ...]" \
		"" \
		Targets: \
		"  make help       Show available targets (default)" \
		"  make deps       Install locked frontend dependencies" \
		"  make generate   Generate Rust-owned Console wire bindings and samples" \
		"  make style      Check Rust and Console formatting without modifying files" \
		"  make format     Format Rust and Console sources" \
		"  make web        Build Console assets for Rust embedding" \
		"  make build      Build the CLI in target/debug/" \
		"  make test       Run socket-free Rust and Console tests" \
		"  make lint       Run lint, type and documentation checks, and verify Console contracts" \
		"  make console    Start the local Console; pass Console arguments via ARGS" \
		"" \
		"Run make deps once per environment and after dependency changes." \
		"Run make web before compiling Rust, after frontend changes, and after switching branches." \
		"" \
		Examples: \
		"  make web && make style test lint build" \
		"  make web && make console ARGS='--listen 127.0.0.1:9924'" \
		"  make deps && make web && cargo install --locked --path ."

deps:
	npm --prefix web ci

generate:
	AIBOX_CONTRACT_DIR="$(CURDIR)/web/src/api/generated" TS_RS_LARGE_INT=number \
		cargo test --locked service::control::contract::tests::export_console_contract -- --ignored --exact

style:
	cargo fmt --check
	npm --prefix web run format:check

format:
	cargo fmt
	npm --prefix web run format

web:
	npm --prefix web run build

build:
	cargo build --locked

test:
	cargo test --locked
	npm --prefix web run test

lint:
	cargo clippy --locked --all-targets -- -D warnings
	npm --prefix web run typecheck
	npm --prefix web run lint
	RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --document-private-items
	@set -eu; \
		aibox_contract_tmp=$$(mktemp -d); \
		trap "rm -rf \"\$$aibox_contract_tmp\"" EXIT; \
		AIBOX_CONTRACT_DIR="$$aibox_contract_tmp" TS_RS_LARGE_INT=number \
			cargo test --locked service::control::contract::tests::export_console_contract -- --ignored --exact; \
		diff -u web/src/api/generated/wire.ts "$$aibox_contract_tmp/wire.ts"; \
		diff -u web/src/api/generated/routes.ts "$$aibox_contract_tmp/routes.ts"; \
		diff -u web/src/api/generated/samples.json "$$aibox_contract_tmp/samples.json"

console:
	cargo run --locked -- console $(ARGS)
