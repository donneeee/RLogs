FROM rust:1.94-bookworm AS builder

WORKDIR /src
COPY . .
RUN cargo build --locked --release -p rlogs-submission-service

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 rlogs \
    && install --directory --owner rlogs --group rlogs /data

COPY --from=builder /src/target/release/rlogs-submission-service /usr/local/bin/rlogs-submission-service

ENV RLOGS_SUBMISSION_DATA=/data \
    RLOGS_SUBMISSION_LISTEN=0.0.0.0:8787 \
    RLOGS_PUBLIC_SITE_URL=https://donneeee.github.io/rlogs-website/

VOLUME ["/data"]
EXPOSE 8787
USER rlogs

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD curl --fail --silent http://127.0.0.1:8787/health > /dev/null || exit 1

ENTRYPOINT ["/usr/local/bin/rlogs-submission-service"]
