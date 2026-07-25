.DEFAULT_GOAL := help

SHELL := /bin/bash
MAKEFLAGS += --silent

.PHONY: help build build-minting test test-minting deploy-testnet deploy-mainnet setup-hooks validate-snapshots clean-snapshots docs-error-codes check-error-codes

help:
	@printf "Usage:\n"
	@printf "  make build              Build all contracts\n"
	@printf "  make test               Run all workspace tests\n"
	@printf "  make deploy-testnet     Deploy to Stellar testnet\n"
	@printf "  make deploy-mainnet     Deploy to Stellar mainnet\n"
	@printf "  make setup-hooks        Install git hooks for the repository\n"
	@printf "  make build-minting      Build the acbu_minting contract\n"
	@printf "  make test-minting       Run tests for the acbu_minting contract\n"
	@printf "  make validate-snapshots Validate test snapshots for staleness\n"
	@printf "  make clean-snapshots    Delete all test snapshots (use before regeneration)\n"
	@printf "  make docs-error-codes   Regenerate docs/ERROR_CODES.md from source\n"
	@printf "  make check-error-codes  Verify docs/ERROR_CODES.md matches source\n"

build:
	@printf "Building all contracts...\n"
	cargo build --target wasm32-unknown-unknown --release

build-minting:
	@printf "Building acbu_minting contract...\n"
	cd acbu_minting && cargo build --target wasm32-unknown-unknown --release

test:
	@printf "Running workspace tests...\n"
	cargo test

test-minting:
	@printf "Running acbu_minting tests...\n"
	cd acbu_minting && cargo test

deploy-testnet:
	@if [ -z "$$STELLAR_SECRET_KEY" ]; then \
		echo "ERROR: STELLAR_SECRET_KEY must be set for deployment."; \
		exit 1; \
	fi
	@printf "Deploying to testnet...\n"
	./scripts/deploy_testnet.sh

deploy-mainnet:
	@if [ -z "$$STELLAR_SECRET_KEY" ]; then \
		echo "ERROR: STELLAR_SECRET_KEY must be set for deployment."; \
		exit 1; \
	fi
	@printf "Deploying to mainnet...\n"
	./scripts/deploy_mainnet.sh

setup-hooks:
	@printf "Setting up git hooks...\n"
	./scripts/setup-git-hooks.sh

validate-snapshots:
	@printf "Validating test snapshots...\n"
	cargo test test_snapshot_validation --package acbu_minting -- --nocapture

clean-snapshots:
	@printf "Cleaning test snapshots...\n"
	rm -rf acbu_minting/test_snapshots/*.json
	@printf "Snapshots cleaned. Regenerate by running: make test-minting\n"

docs-error-codes:
	@printf "Regenerating docs/ERROR_CODES.md...\n"
	python scripts/generate_error_codes.py

check-error-codes:
	@printf "Checking docs/ERROR_CODES.md against source...\n"
	python scripts/generate_error_codes.py --check
