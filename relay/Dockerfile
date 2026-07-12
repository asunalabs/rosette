# Relay container (architecture.md Distribution — gated on T9, now landed:
# queue/epoch/backlog state persists in /data, so restarts and redeploys are
# invisible to clients).
#
# Build from the REPO ROOT (the workspace manifest is needed):
#   docker build -f relay/Dockerfile -t chat-relay .
# Run with a volume for identity + state (both live in /data):
#   docker run -v chat-relay-data:/data -p 7443:7443 chat-relay

FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
# Only the relay's dependency subtree (proto + server deps) is compiled;
# cargo never touches the client crates for -p relay.
RUN cargo build --release -p relay

FROM alpine
RUN adduser -D -H relay
COPY --from=build /src/target/release/relay /usr/local/bin/relay
USER relay
VOLUME /data
WORKDIR /data
EXPOSE 7443
# argv: listen addr, identity file, state db — the latter two on the volume.
ENTRYPOINT ["relay", "0.0.0.0:7443", "/data/relay_identity.der", "/data/relay_state.sqlite3"]
