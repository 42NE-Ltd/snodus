# Dockerfile.oss — snodus-core (open source, MIT)
#
# Builds the standalone open source binary. The image ships with only the
# core migrations (`migrations/core/`) — no premium SQL or premium routes.

FROM rust:1.88-slim AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/snodus-core/Cargo.toml crates/snodus-core/Cargo.toml
COPY crates/snodus-cloud/Cargo.toml crates/snodus-cloud/Cargo.toml
COPY crates ./crates
COPY migrations ./migrations
COPY static ./static
RUN cargo build --release -p snodus-core

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
 && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/snodus /usr/local/bin/snodus
COPY --from=builder /app/migrations/core /app/migrations
COPY --from=builder /app/static/core /app/static
EXPOSE 8080
ENV RUST_LOG=snodus=info,snodus_core=info
CMD ["snodus", "serve"]
