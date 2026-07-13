# Directory container (T18 — the deploy-target-agnostic part: no TLS/LB
# config here, that's picked per cloud target when one is chosen).
#
# Build from the REPO ROOT (the workspace manifest is needed):
#   docker build -f directory/Dockerfile -t chat-directory .
# Run against a real Postgres, with the pepper from a secrets manager/KMS:
#   docker run -p 7444:7444 \
#     -e DATABASE_URL=postgres://... \
#     -e DIRECTORY_PEPPER=... \
#     chat-directory
# Migrations run automatically on startup (DirectoryStore::connect).

FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src
COPY . .
# Only the directory's dependency subtree is compiled; cargo never touches
# the client crates for -p directory.
RUN cargo build --release -p directory

FROM alpine
RUN adduser -D -H directory
COPY --from=build /src/target/release/directory /usr/local/bin/directory
USER directory
EXPOSE 7444
ENV DIRECTORY_ADDR=0.0.0.0:7444
# DATABASE_URL and DIRECTORY_PEPPER are required and have no default — the
# binary refuses to start without them (see main.rs).
ENTRYPOINT ["directory"]
