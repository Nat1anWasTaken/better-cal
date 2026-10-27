# syntax=docker/dockerfile:1.7
# One Dockerfile, two targets (`runner`, `app`) sharing a single Rust build
# stage. When both images are built together (docker compose build), BuildKit
# runs the shared stage once, so only one rustc build uses memory at a time.

FROM debian:bookworm-slim AS isolate-build
ARG ISOLATE_VERSION=v2.7
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates git build-essential libcap-dev libseccomp-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 --branch "${ISOLATE_VERSION}" https://github.com/ioi/isolate /src/isolate
WORKDIR /src/isolate
RUN make isolate isolate-check-environment

FROM rust:1.99-slim-bookworm AS rust-build
# Keep peak memory bounded: at most two rustc processes at once.
ENV CARGO_BUILD_JOBS=2 CARGO_TERM_COLOR=never
WORKDIR /src
COPY backend/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target,id=ccal-target \
    cargo build --release --locked -p ccal-api -p judge-runner \
    && cp target/release/cyculcal target/release/judge-runner /

FROM node:24-bookworm-slim AS web-build
ENV PNPM_HOME=/pnpm PATH=/pnpm:$PATH CI=true
RUN corepack enable
WORKDIR /web
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN --mount=type=cache,target=/pnpm/store pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

# --- judge-runner: pinned GCC 12.2 (Debian 12) + isolate v2.7 ---------------
FROM debian:bookworm-slim AS runner
# The C toolchain students compile against. Bump deliberately and regenerate
# diagnostics fixtures (scripts/gen-fixtures.sh) when changing it.
RUN apt-get update && apt-get install -y --no-install-recommends \
      gcc-12=12.2.0-14+deb12u1 cpp-12=12.2.0-14+deb12u1 libgcc-12-dev=12.2.0-14+deb12u1 \
      libc6-dev libcap2 libseccomp2 ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=isolate-build /src/isolate/isolate /usr/local/bin/isolate
COPY --from=isolate-build /src/isolate/isolate-check-environment /usr/local/bin/isolate-check-environment
RUN chmod 4755 /usr/local/bin/isolate && mkdir -p /var/local/lib/isolate && chmod 700 /var/local/lib/isolate
COPY deploy/isolate.cf /usr/local/etc/isolate
COPY deploy/runner-entrypoint.sh /usr/local/bin/runner-entrypoint.sh
COPY --from=rust-build /judge-runner /usr/local/bin/judge-runner
EXPOSE 7070
HEALTHCHECK --interval=10s --timeout=3s --retries=3 CMD ["/usr/local/bin/judge-runner", "--healthcheck"]
ENTRYPOINT ["/usr/local/bin/runner-entrypoint.sh"]
CMD ["/usr/local/bin/judge-runner"]

# --- app: API + judge queue workers + web UI ----------------------------------
FROM debian:bookworm-slim AS app
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home /app app
WORKDIR /app
COPY --from=rust-build /cyculcal /app/cyculcal
COPY --from=web-build /web/dist /app/web
USER app
ENV BIND=0.0.0.0:8080 STATIC_DIR=/app/web
EXPOSE 8080
CMD ["/app/cyculcal", "serve"]
