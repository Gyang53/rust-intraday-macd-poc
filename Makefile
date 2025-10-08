# Makefile for Rust Intraday MACD POC
.PHONY: help build run test clean fmt lint docker-build docker-up docker-down docker-logs

# Default target
help:
	@echo "Available targets:"
	@echo "  build      - Build the project in release mode"
	@echo "  run        - Run the application"
	@echo "  test       - Run tests"
	@echo "  clean      - Clean build artifacts"
	@echo "  fmt        - Format code"
	@echo "  lint       - Run clippy linter"
	@echo "  docker-build - Build Docker image"
	@echo "  docker-up  - Start services with Docker Compose"
	@echo "  docker-down - Stop services with Docker Compose"
	@echo "  docker-logs - Show Docker Compose logs"

# Build targets
build:
	@echo "Building project..."
	cargo build --release

run:
	@echo "Running application..."
	cargo run

run-dev:
	@echo "Running in development mode..."
	RUST_LOG=debug cargo run

run-test:
	@echo "Running in test mode..."
	RUN_MODE=test RUST_LOG=debug cargo run

# Testing targets
test:
	@echo "Running tests..."
	cargo test

test-verbose:
	@echo "Running tests with verbose output..."
	cargo test -- --nocapture

test-coverage:
	@echo "Running tests with coverage..."
	cargo tarpaulin --out Html

# Code quality targets
fmt:
	@echo "Formatting code..."
	cargo fmt

lint:
	@echo "Running clippy..."
	cargo clippy -- -D warnings

check:
	@echo "Running cargo check..."
	cargo check

# Clean targets
clean:
	@echo "Cleaning build artifacts..."
	cargo clean
	rm -f trading.db trading.db-* test_trading.db test_trading.db-* logs/*.log

clean-docker:
	@echo "Cleaning Docker artifacts..."
	docker-compose down -v --remove-orphans
	docker system prune -f

# Docker targets
docker-build:
	@echo "Building Docker image..."
	docker-compose build

docker-up:
	@echo "Starting services..."
	docker-compose up -d

docker-down:
	@echo "Stopping services..."
	docker-compose down

docker-logs:
	@echo "Showing logs..."
	docker-compose logs -f

docker-restart:
	@echo "Restarting services..."
	docker-compose restart

# Development workflow targets
dev: fmt lint test build
	@echo "Development workflow completed"

dev-setup: docker-up
	@echo "Development environment setup completed"

# Database targets
db-reset:
	@echo "Resetting database..."
	rm -f trading.db trading.db-*
	cargo run -- --gen-sim

db-test-reset:
	@echo "Resetting test database..."
	rm -f test_trading.db test_trading.db-*
	RUN_MODE=test cargo run -- --gen-sim

# Monitoring targets
monitor:
	@echo "Starting monitoring..."
	cargo run &
	@echo "Application started. Press Ctrl+C to stop."
	@sleep 2
	curl http://localhost:8080/api/health
	@echo ""
	@wait

# Release targets
release: clean fmt lint test build
	@echo "Release build completed"

release-docker: docker-build
	@echo "Docker release build completed"

# Documentation targets
docs:
	@echo "Generating documentation..."
	cargo doc --no-deps --open

# Benchmark targets
bench:
	@echo "Running benchmarks..."
	cargo bench

# Install dependencies
deps:
	@echo "Installing dependencies..."
	cargo update

# Environment setup
env-setup:
	@echo "Setting up development environment..."
	rustup component add rustfmt
	rustup component add clippy
	cargo install cargo-tarpaulin
	cargo install cargo-watch

# Watch for changes and run tests
watch:
	@echo "Watching for changes..."
	cargo watch -x test

watch-run:
	@echo "Watching for changes and running..."
	cargo watch -x run

# Profile targets
profile:
	@echo "Building for profiling..."
	cargo build --release
	@echo "Run with: ./target/release/rust-intraday-macd-poc"

# Security audit
audit:
	@echo "Running security audit..."
	cargo audit

# Size analysis
size:
	@echo "Analyzing binary size..."
	cargo bloat --release
	cargo size --release

# Cross-compilation (example for Linux)
cross-linux:
	@echo "Cross-compiling for Linux..."
	cargo build --release --target x86_64-unknown-linux-musl

# Backup targets
backup:
	@echo "Creating backup..."
	tar -czf backup-$(shell date +%Y%m%d-%H%M%S).tar.gz src/ config/ static/ Cargo.toml README.md

# Deployment targets
deploy-staging: release-docker
	@echo "Deploying to staging..."
	# Add your deployment commands here

deploy-production: release-docker
	@echo "Deploying to production..."
	# Add your deployment commands here

# Default target
.DEFAULT_GOAL := help
