FROM rust:1.99-bookworm AS builder
WORKDIR /workspace
COPY Cargo.toml Cargo.lock ./
COPY apps/desktop/Cargo.toml apps/desktop/Cargo.toml
COPY apps/api apps/api
COPY crates crates
COPY tools tools
COPY vendor vendor
RUN mkdir -p apps/desktop/src && printf 'fn main() {}\n' > apps/desktop/src/main.rs && printf 'fn main() {}\n' > apps/desktop/build.rs \
    && mkdir -p apps/desktop/tests && printf 'fn main() {}\n' > apps/desktop/tests/macos_text.rs
RUN cargo build --locked --release -p lennette-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home lennette \
    && mkdir -p /data && chown lennette:lennette /data
COPY --from=builder /workspace/target/release/lennette-server /usr/local/bin/lennette-server
USER lennette
WORKDIR /data
EXPOSE 8090
ENTRYPOINT ["lennette-server"]
