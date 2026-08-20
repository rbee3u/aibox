.DEFAULT_GOAL := help

.PHONY: help install format build test lint check \
	console-ci console-format console-build console-test console-lint console-check

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
		"  make console-check      Run all frontend checks and build assets"

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
	npm --prefix console run check
