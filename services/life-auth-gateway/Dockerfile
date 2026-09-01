# syntax=docker/dockerfile:1.7
FROM rust:1.88-bookworm AS build
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release -p life-auth-gateway && \
    cp /src/target/release/life-auth-gateway /tmp/life-auth-gateway

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=build /tmp/life-auth-gateway /usr/local/bin/life-auth-gateway
USER 65532:65532
EXPOSE 3200
ENTRYPOINT ["life-auth-gateway"]
