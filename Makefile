.PHONY: help install format build test lint check rust-check dev \
	web-ci web-build web-check web-contract web-contract-check

help:
	@printf '%s\n' \
		"Project:" \
		"  make install             Install frontend dependencies, build, and install the CLI" \
		"  make dev                 Build and run the local Console" \
		"  make format              Format Rust and Console sources" \
		"  make build               Build Console assets and the CLI" \
		"  make test                Run Rust and Console tests" \
		"  make lint                Lint Rust and Console sources" \
		"  make check               Run all socket-free project checks" \
		"Rust:" \
		"  make rust-check          Build Console assets and run all Rust checks" \
		"Web (Console):" \
		"  make web-ci              Install frontend dependencies with npm ci" \
		"  make web-build           Build embedded frontend assets" \
		"  make web-check           Run frontend checks and Rust-owned contract verification" \
		"  make web-contract        Update Rust-owned wire bindings and samples" \
		"  make web-contract-check  Verify committed Rust-owned wire contracts" \
		"All tasks that compile Rust first build Console assets. Only install and web-ci install dependencies."

install: web-ci
	npm --prefix web run build
	cargo install --locked --path .

format:
	cargo fmt
	npm --prefix web run format

build: web-build
	cargo build --locked

test: web-build
	cargo test --locked
	npm --prefix web run test

lint: web-build
	cargo clippy --locked --all-targets -- -D warnings
	npm --prefix web run lint

check: rust-check web-check

rust-check: web-build
	cargo fmt --check
	cargo test --locked
	cargo clippy --locked --all-targets -- -D warnings
	RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --document-private-items

web-ci:
	npm --prefix web ci

web-build:
	npm --prefix web run build

web-check: web-contract-check
	npm --prefix web run format:check
	npm --prefix web run typecheck
	npm --prefix web run test
	npm --prefix web run lint

web-contract: web-build
	AIBOX_CONTRACT_DIR="$(CURDIR)/web/src/api/generated" TS_RS_LARGE_INT=number \
		cargo test --locked service::control::contract::tests::export_console_contract -- --ignored --exact

web-contract-check: web-build
	@aibox_contract_tmp="$$(mktemp -d)"; \
		trap 'rm -rf "$$aibox_contract_tmp"' EXIT; \
		AIBOX_CONTRACT_DIR="$$aibox_contract_tmp" TS_RS_LARGE_INT=number \
			cargo test --locked service::control::contract::tests::export_console_contract -- --ignored --exact; \
		diff -u web/src/api/generated/wire.ts "$$aibox_contract_tmp/wire.ts"; \
		diff -u web/src/api/generated/routes.ts "$$aibox_contract_tmp/routes.ts"; \
		diff -u web/src/api/generated/samples.json "$$aibox_contract_tmp/samples.json"

dev: web-build
	cargo run --locked -- console
