FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY migrations ./migrations
COPY src ./src
COPY templates ./templates
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
RUN useradd -m -u 10001 watchlist
WORKDIR /app
COPY --from=builder /app/target/release/watchlist /usr/local/bin/watchlist
COPY static ./static
RUN mkdir -p /data && chown -R watchlist:watchlist /data /app
USER watchlist

ENV WATCHLIST_DB=/data/watchlist.db
ENV WATCHLIST_HOST=0.0.0.0
ENV WATCHLIST_PORT=3000
EXPOSE 3000
VOLUME ["/data"]

CMD ["watchlist"]
