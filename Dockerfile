FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY publish-linux/claude-api-proxy /app/claude-api-proxy
COPY publish-linux/static /app/static

RUN chmod +x /app/claude-api-proxy

EXPOSE 8000

ENTRYPOINT ["/app/claude-api-proxy"]
