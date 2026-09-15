# Suwayomi (next) — headless server image (Phase 7).
# Default backend is a local SQLite file; set SUWAYOMI_DATABASE_URL (and
# SUWAYOMI_DB_BACKEND=postgres) to point at an external PostgreSQL instead.

FROM rust:1.95-slim AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev perl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . .
RUN cargo build --release --bin suwayomi-server

# GLIBC must match the build image (rust:1.95-slim is trixie-based).
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/suwayomi-server /usr/local/bin/suwayomi-server
EXPOSE 8090
ENV SUWAYOMI_PORT=8090 \
    SUWAYOMI_IP=0.0.0.0 \
    SUWAYOMI_DATA_DIR=/data
VOLUME ["/data"]
CMD ["suwayomi-server"]
