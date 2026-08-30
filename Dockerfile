# Suwayomi (next) — headless server image (Phase 7).
# Embedded PGlite backend: zero external dependencies at runtime.

FROM rust:1.95-slim AS build
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev perl \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY . .
RUN cargo build --release --bin suwayomi

# GLIBC must match the build image (rust:1.95-slim is trixie-based).
FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /app/target/release/suwayomi /usr/local/bin/suwayomi
EXPOSE 4567
ENV SUWAYOMI_PORT=4567 \
    SUWAYOMI_IP=0.0.0.0 \
    SUWAYOMI_PGLITE_DATA_DIR=/data/pglite-data
VOLUME ["/data"]
CMD ["suwayomi"]
