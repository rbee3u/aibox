.DEFAULT_GOAL := help

.PHONY: help install format build test lint check \
	traffic-ci traffic-format traffic-build traffic-test traffic-lint traffic-check

help:
	@printf '%s\n' \
		"Rust:" \
		"  make install            Install the CLI with Cargo" \
		"  make format             Format Rust sources" \
		"  make build              Build the CLI" \
		"  make test               Run Rust tests" \
		"  make lint               Lint Rust sources" \
		"  make check              Run all Rust checks" \
		"Traffic UI:" \
		"  make traffic-ci         Install frontend dependencies with npm ci" \
		"  make traffic-format     Format frontend sources" \
		"  make traffic-build      Build embedded frontend assets" \
		"  make traffic-test       Run frontend tests" \
		"  make traffic-lint       Lint frontend sources" \
		"  make traffic-check      Run all frontend checks and build assets"

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

traffic-ci:
	npm --prefix web/traffic ci

traffic-format:
	npm --prefix web/traffic run format

traffic-build:
	npm --prefix web/traffic run build

traffic-test:
	npm --prefix web/traffic run test

traffic-lint:
	npm --prefix web/traffic run lint

traffic-check:
	npm --prefix web/traffic run check
