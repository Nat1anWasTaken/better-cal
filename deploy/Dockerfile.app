# syntax=docker/dockerfile:1.7
# CYCU-CAL app image: Rust API (+ judge queue workers) serving the built SPA.

FROM node:24-bookworm-slim AS web-build
ENV PNPM_HOME=/pnpm PATH=/pnpm:$PATH CI=true
RUN corepack enable
WORKDIR /web
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN --mount=type=cache,target=/pnpm/store pnpm install --frozen-lockfile
COPY frontend/ ./
COPY docs/openapi.json /docs/openapi.json
RUN pnpm build

FROM rust:1.99-slim-bookworm AS api-build
ENV CARGO_BUILD_JOBS=2 CARGO_TERM_COLOR=never
WORKDIR /src
COPY backend/ ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target,id=ccal-app-target \
    cargo build --release --locked -p ccal-api \
    && cp target/release/cyculcal /cyculcal

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home /app app
WORKDIR /app
COPY --from=api-build /cyculcal /app/cyculcal
COPY --from=web-build /web/dist /app/web
USER app
ENV BIND=0.0.0.0:8080 STATIC_DIR=/app/web
EXPOSE 8080
CMD ["/app/cyculcal", "serve"]
