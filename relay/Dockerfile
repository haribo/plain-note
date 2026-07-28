# Relay container image — a release artifact, not a hosted service.
# Build from the repo root: docker build -f relay/Dockerfile .
#
# Configuration is entirely via environment (see docs/relay usage / README):
#   PN_RELAY_BIND (default 0.0.0.0:8787), PN_RELAY_DB, PN_RELAY_ADMIN_TOKEN.
# All durable state (incl. attachment blobs) lives in the SQLite file under
# /data — mount a volume there.

# --- build stage ---
FROM rust:1-bookworm AS builder
WORKDIR /src
COPY . .
# Snapshot pipeline passes develop-<date>-<sha7>; falls back to git/dev otherwise.
ARG PN_BUILD_INFO=""
ENV PN_BUILD_INFO=${PN_BUILD_INFO}
RUN cargo build --release -p note-relay

# --- runtime stage ---
FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --user-group relay \
    && mkdir -p /data && chown relay:relay /data
COPY --from=builder /src/target/release/pn-relay /usr/local/bin/pn-relay
USER relay
ENV PN_RELAY_BIND=0.0.0.0:8787 \
    PN_RELAY_DB=/data/relay.db
EXPOSE 8787
VOLUME ["/data"]
ENTRYPOINT ["/usr/local/bin/pn-relay"]
