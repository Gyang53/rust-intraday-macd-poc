# Multi-stage Dockerfile for Rust intraday MACD POC
FROM rust:1.70-alpine AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static

# Create app directory
WORKDIR /app

# Copy source files
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY config/ ./config/
COPY static/ ./static/

# Build the application
RUN cargo build --release

# Runtime stage
FROM alpine:latest

# Install runtime dependencies
RUN apk add --no-cache openssl ca-certificates

# Create non-root user
RUN addgroup -S appgroup && adduser -S appuser -G appgroup

# Create app directory
WORKDIR /app

# Copy binary from builder stage
COPY --from=builder /app/target/release/rust-intraday-macd-poc /app/
COPY --from=builder /app/config/ /app/config/
COPY --from=builder /app/static/ /app/static/

# Create data directory
RUN mkdir -p /app/data && chown -R appuser:appgroup /app

# Switch to non-root user
USER appuser

# Expose port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:8080/api/health || exit 1

# Set environment variables
ENV RUST_LOG=info
ENV RUN_MODE=production

# Run the application
CMD ["./rust-intraday-macd-poc"]
