# syntax=docker/dockerfile:1.7
FROM rust:1.88-bookworm AS build
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --locked --release \
      -p business-auth-gateway \
      -p business-core \
      -p business-read-api \
      -p buzz-acp \
      -p buzz-agent \
      -p business-read-mcp && \
    cp /src/target/release/business-auth-gateway /tmp/business-auth-gateway && \
    cp /src/target/release/business-core /tmp/business-core && \
    cp /src/target/release/business-read-api /tmp/business-read-api && \
    cp /src/target/release/buzz-acp /tmp/buzz-acp && \
    cp /src/target/release/buzz-agent /tmp/buzz-agent && \
    cp /src/target/release/business-read-mcp /tmp/business-read-mcp

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
USER 65532:65532

FROM runtime AS gateway
COPY --from=build /tmp/business-auth-gateway /usr/local/bin/business-auth-gateway
EXPOSE 3100
ENTRYPOINT ["business-auth-gateway"]

FROM runtime AS business-core
COPY --from=build /tmp/business-core /usr/local/bin/business-core
EXPOSE 3120
ENTRYPOINT ["business-core"]

FROM runtime AS business-read-api
COPY --from=build /tmp/business-read-api /usr/local/bin/business-read-api
EXPOSE 3130
ENTRYPOINT ["business-read-api"]

FROM runtime AS proxy-agent
COPY --from=build /tmp/buzz-acp /usr/local/bin/buzz-acp
COPY --from=build /tmp/buzz-agent /usr/local/bin/buzz-agent
COPY --from=build /tmp/business-read-mcp /usr/local/bin/business-read-mcp
ENTRYPOINT ["buzz-acp"]
