# ---- Build Stage ----
FROM rust:1.93-bookworm AS builder

WORKDIR /build

# Copy workspace manifests first for layer caching.
COPY Cargo.toml Cargo.lock ./
COPY os-llm/Cargo.toml os-llm/Cargo.toml
COPY os-tools/Cargo.toml os-tools/Cargo.toml
COPY os-channels/Cargo.toml os-channels/Cargo.toml
COPY os-app/Cargo.toml os-app/Cargo.toml

# Dummy source files so cargo can compute deps.
RUN mkdir -p os-llm/src os-tools/src os-channels/src os-app/src && \
    echo "fn main() {}" > os-app/src/main.rs && \
    touch os-llm/src/lib.rs os-tools/src/lib.rs os-channels/src/lib.rs

# rusqlite and libsql both bundle sqlite3.c; allow the duplicate symbols.
ENV RUSTFLAGS="-C link-args=-Wl,--allow-multiple-definition"

# Fetch + compile deps (cached unless Cargo.toml changes).
# Horizons is fetched automatically as a git dependency.
RUN cargo build --release --workspace 2>/dev/null || true

# Copy real source.
COPY . .

# Touch source files to invalidate the dummy build.
RUN find . -name "*.rs" -exec touch {} +

RUN cargo build --release --bin opencraw

# ---- Runtime Stage ----
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/opencraw /usr/local/bin/opencraw

RUN useradd -m opencraw
USER opencraw
WORKDIR /home/opencraw

EXPOSE 3000

ENTRYPOINT ["opencraw"]
CMD ["serve"]
