FROM rust:1.83-slim AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src ./src
RUN cargo build --release && strip target/release/pinapi

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/pinapi /app/pinapi

ENV PINAPI_CRED_ROOT=/data/creds \
    PINAPI_UPLOAD_DIR=/data/uploads
VOLUME /data
EXPOSE 8000

CMD ["/app/pinapi"]
