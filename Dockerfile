FROM rust:1.67 as builder

WORKDIR /usr/src/app
COPY . .
# Will build and cache the binary and dependent crates in release mode
RUN --mount=type=cache,target=/usr/local/cargo,from=rust:latest,source=/usr/local/cargo \
    --mount=type=cache,target=target \
    cargo build --release && mv ./target/release/vss-rs ./vss-rs

# Runtime image
FROM debian:bullseye-slim

RUN apt update && apt install -y openssl libpq-dev pkg-config libc6 openssl libssl-dev libpq5 ca-certificates

# Run as "app" user
RUN useradd -ms /bin/bash app

USER app
WORKDIR /app

# Get compiled binaries from builder's cargo install directory
COPY --from=builder /usr/src/app/vss-rs /app/vss-rs

ENV VSS_PORT=8080
EXPOSE $VSS_PORT

# Run the app
CMD ./vss-rs