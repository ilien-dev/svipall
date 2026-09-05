# svipall in a container: the MCP server on stdio, the dashboard and solver API on 8787, and a
# Chrome for Testing that the browser tiers can drive. Everything stays inside the container and
# the volume; nothing phones home.
#
#   docker build -t svipall .
#   docker run -i --rm -v svipall-home:/data svipall                    # MCP over stdio
#   docker run --rm -p 8787:8787 -v svipall-home:/data svipall           # dashboard reachable
#   docker run --rm -v svipall-home:/data svipall svipall fetch https://example.com
#
# The REST API is off unless asked for, in the image as everywhere else — it grants everything the
# MCP tools do. Opt in with a port and a key of your own:
#
#   docker run --rm -p 8788:8788 -e SVIPALL_REST_PORT=8788 -e SVIPALL_API_KEY=… \
#     -v svipall-home:/data svipall
#
# Note the bind: `rest_bind` defaults to loopback, which inside a container means the container. Set
# it to 0.0.0.0 in /data/config.toml to publish the port, and read what the README says about doing
# that before you do.
#
# `-i` keeps stdin open, which is what an MCP client needs. Point the client at
# `docker run -i --rm -v svipall-home:/data svipall` as the command.

FROM rust:1-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential cmake perl pkg-config libclang-dev nasm \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# A portable baseline rather than the host's CPU: the image runs on machines it was not built on.
ENV RUSTFLAGS="-C target-cpu=x86-64-v2"
RUN case "$(uname -m)" in aarch64) export RUSTFLAGS="" ;; esac; \
    cargo build --profile dist --bin svipall-mcp --bin svipall \
    && mkdir -p /out && cp target/dist/svipall-mcp target/dist/svipall /out/

FROM debian:bookworm-slim
# What Chrome for Testing needs to start headless, and nothing it does not.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates fonts-liberation libasound2 libatk-bridge2.0-0 libatk1.0-0 libcups2 \
        libdbus-1-3 libdrm2 libgbm1 libglib2.0-0 libnspr4 libnss3 libpango-1.0-0 libx11-6 \
        libxcomposite1 libxdamage1 libxext6 libxfixes3 libxkbcommon0 libxrandr2 xdg-utils \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/svipall-mcp /out/svipall /usr/local/bin/
# Everything svipall learns, caches and keeps lives here; mount a volume to keep it.
ENV SVIPALL_HOME=/data
# The dashboard binds inside the container; publish 8787 to reach it from outside.
ENV SVIPALL_DASHBOARD_PORT=8787
RUN useradd --create-home --uid 1000 svipall && mkdir -p /data && chown svipall /data
USER svipall
# A browser of its own, so the browser tiers work out of the box. Downloaded at build time,
# never at run time.
RUN svipall browser install || echo "browser install skipped; http tier only"
VOLUME ["/data"]
# 8787 is the dashboard and solver API; 8788 is the REST API, which stays off unless
# SVIPALL_REST_PORT says otherwise.
EXPOSE 8787 8788
ENTRYPOINT ["svipall-mcp"]
