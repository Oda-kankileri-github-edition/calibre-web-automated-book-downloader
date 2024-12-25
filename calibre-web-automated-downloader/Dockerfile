# Use rust image as base
FROM rust:bullseye AS builder

# Set working directory
WORKDIR /app

# Copy the source code into the container
COPY calibre-web-automated-downloader .

# Build the application
RUN cargo build --release

# Create a new image
FROM debian:bullseye-slim

# Set environment variables
ENV DEBIAN_FRONTEND=noninteractive
ENV DOCKERMODE=true
ENV NAME=Calibre-Web-Automated-Book-Downloader
ENV HOST=0.0.0.0
ENV PORT=8084
ENV CLOUDFLARE_PROXY_URL=http://localhost:8000
ENV INGEST_DIR=/cwa-book-ingest
ENV STATUS_TIMEOUT=3600
ENV USE_CF_BYPASS=true
ENV AA_BASE_URL=https://annas-archive.org

# Default UID and GID (can be overridden at runtime)
ENV UID=1000
ENV GID=100

# Set working directory
WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends --no-install-suggests \
    calibre p7zip curl gosu \
    && rm -rf /var/lib/apt/lists/*

COPY . .
COPY --from=builder /app/target/release/calibre-web-automated-downloader /app/app

RUN chmod +x /app/check_health.sh && \
    chmod +x /app/entrypoint.sh

# Expose port
EXPOSE ${PORT}

# Health check
HEALTHCHECK --interval=30s --timeout=30s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:${PORT}/request/api/status || exit 1

# Entrypoint
ENTRYPOINT ["/app/entrypoint.sh"]

# Start application
CMD ["./app"]
