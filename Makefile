.DEFAULT_GOAL := help

.PHONY: help install format build test lint check \
	console-ci console-format console-build console-test console-lint console-check \
	console-contract console-contract-check console-assets-check

help:
	@printf '%s\n' \
		"Rust:" \
		"  make install            Install the CLI with Cargo" \
		"  make format             Format Rust sources" \
		"  make build              Build the CLI" \
		"  make test               Run Rust tests" \
		"  make lint               Lint Rust sources" \
		"  make check              Run all Rust checks" \
		"Console UI:" \
		"  make console-ci         Install frontend dependencies with npm ci" \
		"  make console-format     Format frontend sources" \
		"  make console-build      Build embedded frontend assets" \
		"  make console-test       Run frontend tests" \
		"  make console-lint       Lint frontend sources" \
		"  make console-check      Run all socket-free frontend and contract checks" \
		"  make console-contract   Update Rust-owned wire bindings and samples" \
		"  make console-contract-check  Verify committed Rust-owned wire contracts" \
		"  make console-assets-check    Verify committed embedded Console assets"

install:
	cargo install --locked --path .

format:
	cargo fmt

build:
	cargo build

test:
	cargo test

lint:
	cargo clippy --all-targets -- -D warnings

check:
	cargo fmt --check
	cargo test
	cargo clippy --all-targets -- -D warnings

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

console-check:
	npm --prefix console run format:check
	npm --prefix console run typecheck
	npm --prefix console run test
	npm --prefix console run lint
	$(MAKE) console-contract-check
	$(MAKE) console-assets-check

console-contract:
	AIBOX_CONTRACT_DIR="$(CURDIR)/console/src/api/generated" TS_RS_LARGE_INT=number \
		cargo test service::control::contract::tests::export_console_contract -- --ignored --exact

console-contract-check:
	@aibox_contract_tmp="$$(mktemp -d)"; \
		trap 'rm -rf "$$aibox_contract_tmp"' EXIT; \
		AIBOX_CONTRACT_DIR="$$aibox_contract_tmp" TS_RS_LARGE_INT=number \
			cargo test service::control::contract::tests::export_console_contract -- --ignored --exact; \
		diff -u console/src/api/generated/wire.ts "$$aibox_contract_tmp/wire.ts"; \
		diff -u console/src/api/generated/routes.ts "$$aibox_contract_tmp/routes.ts"; \
		diff -u console/src/api/generated/samples.json "$$aibox_contract_tmp/samples.json"

console-assets-check:
	@aibox_assets_tmp="$$(mktemp -d)"; \
		trap 'rm -rf "$$aibox_assets_tmp"' EXIT; \
		AIBOX_CONSOLE_OUT_DIR="$$aibox_assets_tmp" npm --prefix console run build; \
		for aibox_asset in console.html console.css console.js; do \
			diff -u "assets/$$aibox_asset" "$$aibox_assets_tmp/$$aibox_asset"; \
		done
