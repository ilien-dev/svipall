# svipall in a container: the MCP server on stdio, the dashboard and solver API on 8787, and a
# Chrome for Testing that the browser tiers can drive. Everything stays inside the container and
# the volume; nothing phones home.
#
#   docker build -t svipall .                          # full: browser, models, every tier
#   docker build -t svipall:slim --build-arg FLAVOR=slim .   # http tier only, no browser
#
#   docker run -i --rm -v svipall-home:/data svipall                     # MCP over stdio
#   docker run --rm -p 8787:8787 -v svipall-home:/data svipall           # dashboard reachable
#   docker run --rm -v svipall-home:/data svipall svipall fetch https://example.com
#   docker run --rm -v svipall-home:/data svipall svipall doctor         # what this image can do
#
# Two flavours, and the difference is real rather than cosmetic. `full` carries Chrome for Testing
# and the captcha models, so the tier ladder and local solving both work. `slim` carries neither:
# it is the http tier, and a page behind a challenge stays blocked. `slim` is also the only one
# built for arm64, because Chrome for Testing publishes no linux-arm64 build — an arm64 "full"
# image would be a full image with no browser in it, which is a worse thing to ship than an
# honest slim one.
#
# The REST API is off unless asked for, in the image as everywhere else — it grants everything the
# MCP tools do. Opt in with a port and a key of your own:
#
#   docker run --rm -p 8788:8788 -e SVIPALL_REST_PORT=8788 -e SVIPALL_API_KEY=… \
#     -v svipall-home:/data svipall
#
# On first start the entrypoint writes /data/config.toml if there is none, binding the dashboard
# and the REST API to 0.0.0.0. Inside a container that is what makes `-p 8787:8787` reach anything
# at all: loopback here means this container, not your machine. The container's own network is the
# boundary — nothing is reachable until you publish a port — and the REST API additionally stays
# off until SVIPALL_REST_PORT says otherwise, behind a bearer key. Your own /data/config.toml is
# never overwritten.
#
# `-i` keeps stdin open, which is what an MCP client needs. Point the client at
# `docker run -i --rm -v svipall-home:/data svipall` as the command.

ARG FLAVOR=full

# ---------------------------------------------------------------------------------------------
FROM rust:1-bookworm AS build
ARG FLAVOR
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential cmake perl pkg-config libclang-dev nasm \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .

# The models the binary carries, exported from torchvision's published weights (BSD-3) exactly as
# the release workflow does it. Without this step the image builds fine and then answers no image
# captcha at all, silently — the image used to claim the models the tarballs have and not have
# them. The CPU wheel is enough: exporting is not training.
RUN if [ "$FLAVOR" = "full" ]; then \
        apt-get update && apt-get install -y --no-install-recommends python3 python3-pip \
        && rm -rf /var/lib/apt/lists/* \
        && pip3 install --quiet --break-system-packages torch torchvision \
             --index-url https://download.pytorch.org/whl/cpu \
             --extra-index-url https://pypi.org/simple \
        && pip3 install --quiet --break-system-packages onnx \
        && python3 tools/models/export.py \
        && ls -la crates/svipall-models/models ; \
    fi

# A portable baseline rather than the host's CPU: the image runs on machines it was not built on.
ENV RUSTFLAGS="-C target-cpu=x86-64-v2"
RUN case "$(uname -m)" in aarch64) export RUSTFLAGS="" ;; esac; \
    if [ "$FLAVOR" = "full" ]; then \
        features="--features onnx-ocr,onnx-grid,onnx-audio,onnx-detect,onnx-segment,onnx-zeroshot"; \
    else \
        features=""; \
    fi; \
    cargo build --profile dist --bin svipall-mcp --bin svipall $features \
    && mkdir -p /out && cp target/dist/svipall-mcp target/dist/svipall /out/

# ---------------------------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime-base
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/svipall-mcp /out/svipall /usr/local/bin/
COPY docker-entrypoint.sh /usr/local/bin/
# Everything svipall learns, caches and keeps lives here; mount a volume to keep it.
ENV SVIPALL_HOME=/data
# The dashboard binds inside the container; publish 8787 to reach it from outside.
ENV SVIPALL_DASHBOARD_PORT=8787
RUN chmod +x /usr/local/bin/docker-entrypoint.sh \
    && useradd --create-home --uid 1000 svipall && mkdir -p /data && chown svipall /data

# ---------------------------------------------------------------------------------------------
FROM runtime-base AS runtime-full
# What Chrome for Testing needs to start headless, and nothing it does not.
RUN apt-get update && apt-get install -y --no-install-recommends \
        fonts-liberation libasound2 libatk-bridge2.0-0 libatk1.0-0 libcups2 \
        libdbus-1-3 libdrm2 libgbm1 libglib2.0-0 libnspr4 libnss3 libpango-1.0-0 libx11-6 \
        libxcomposite1 libxdamage1 libxext6 libxfixes3 libxkbcommon0 libxrandr2 xdg-utils \
    && rm -rf /var/lib/apt/lists/*
USER svipall
# A browser of its own, so the browser tiers work out of the box. Downloaded at build time,
# never at run time. Fatal on purpose: an image that quietly became http-only is the failure this
# whole flavour exists to prevent, and `slim` is the supported way to ask for one.
RUN svipall browser install

# ---------------------------------------------------------------------------------------------
FROM runtime-base AS runtime-slim
USER svipall

# ---------------------------------------------------------------------------------------------
FROM runtime-${FLAVOR}
VOLUME ["/data"]
# 8787 is the dashboard and solver API; 8788 is the REST API, which stays off unless
# SVIPALL_REST_PORT says otherwise.
EXPOSE 8787 8788
ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["svipall-mcp"]
