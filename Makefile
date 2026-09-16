.DEFAULT_GOAL := help

# Keep dependency installation, asset generation, and checks sequential.
.NOTPARALLEL:

.PHONY: help install format build test lint check \
	rust-format rust-build rust-test rust-lint rust-doc-check rust-check \
	console-ci console-format console-build console-test console-lint console-check \
	console-contract console-contract-check

help:
	@printf '%s\n' \
		"Project:" \
		"  make install            Install frontend dependencies, build, and install the CLI" \
		"  make format             Format Rust and Console sources" \
		"  make build              Build Console assets and the CLI" \
		"  make test               Run Rust and Console tests" \
		"  make lint               Lint Rust and Console sources" \
		"  make check              Run all socket-free project checks" \
		"Rust:" \
		"  make rust-format        Format Rust sources" \
		"  make rust-build         Build the CLI" \
		"  make rust-test          Run Rust tests" \
		"  make rust-lint          Lint Rust sources" \
		"  make rust-doc-check     Check private-item Rust documentation" \
		"  make rust-check         Run all Rust checks" \
		"Console UI:" \
		"  make console-ci         Install frontend dependencies with npm ci" \
		"  make console-format     Format frontend sources" \
		"  make console-build      Build embedded frontend assets" \
		"  make console-test       Run frontend tests" \
		"  make console-lint       Lint frontend sources" \
		"  make console-check      Run all socket-free frontend and contract checks" \
		"  make console-contract   Update Rust-owned wire bindings and samples" \
		"  make console-contract-check  Verify committed Rust-owned wire contracts"

install: console-ci
	npm --prefix console run build
	cargo install --locked --path .

format: rust-format console-format

build: rust-build

test: rust-test console-test

lint: rust-lint console-lint

check: rust-check console-check

rust-format:
	cargo fmt

rust-build: console-build
	cargo build --locked

rust-test: console-build
	cargo test --locked

rust-lint: console-build
	cargo clippy --locked --all-targets -- -D warnings

rust-doc-check: console-build
	RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --document-private-items

rust-check: console-build
	cargo fmt --check
	cargo test --locked
	cargo clippy --locked --all-targets -- -D warnings
	RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --document-private-items

console-ci:
	npm --prefix console ci

console-format:
	npm --prefix console run format

console-build:
	npm --prefix console run build

console-test:
	npm --prefix console run test

console-lint:
	npm --prefix console run lint

console-check: console-contract-check
	npm --prefix console run format:check
	npm --prefix console run typecheck
	npm --prefix console run test
	npm --prefix console run lint

console-contract: console-build
	AIBOX_CONTRACT_DIR="$(CURDIR)/console/src/api/generated" TS_RS_LARGE_INT=number \
		cargo test --locked service::control::contract::tests::export_console_contract -- --ignored --exact

console-contract-check: console-build
	@aibox_contract_tmp="$$(mktemp -d)"; \
		trap 'rm -rf "$$aibox_contract_tmp"' EXIT; \
		AIBOX_CONTRACT_DIR="$$aibox_contract_tmp" TS_RS_LARGE_INT=number \
			cargo test --locked service::control::contract::tests::export_console_contract -- --ignored --exact; \
		diff -u console/src/api/generated/wire.ts "$$aibox_contract_tmp/wire.ts"; \
		diff -u console/src/api/generated/routes.ts "$$aibox_contract_tmp/routes.ts"; \
		diff -u console/src/api/generated/samples.json "$$aibox_contract_tmp/samples.json"
