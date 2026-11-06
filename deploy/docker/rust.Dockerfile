# syntax=docker/dockerfile:1
#
# The aggregator's Rust images. Build from the repository root:
#
#   docker build -f deploy/docker/rust.Dockerfile --target server  -t pacod-agg .
#   docker build -f deploy/docker/rust.Dockerfile --target scraper -t pacod-agg-scraper .
#   docker build -f deploy/docker/rust.Dockerfile --target mock    -t pacod-agg-mock .
#
# The build context is narrowed by rust.Dockerfile.dockerignore (next to this file) to the Cargo
# workspace. As pacod-gw's: cargo-chef keeps third-party crates in their own layer, the binaries
# are linked statically against musl (mimalloc replaces musl's allocator) and run on a distroless
# base without a shell or package manager, as user 65532.

ARG RUST_VERSION=1.99.0
ARG CARGO_CHEF_VERSION=0.1.78
# The tag "nonroot" moves; the digest pins it (the one pacod-gw pins). Refresh with:
#   docker buildx imagetools inspect gcr.io/distroless/static-debian13:nonroot
ARG RUNTIME_IMAGE=gcr.io/distroless/static-debian13:nonroot@sha256:e2e927ec666bae08560abb3c55d0659eceabb657f56b6782ab500a9fc7f555e3

FROM lukemathwalker/cargo-chef:${CARGO_CHEF_VERSION}-rust-${RUST_VERSION}-slim-trixie AS chef
RUN apt-get update \
 && apt-get install -y --no-install-recommends musl-tools \
 && rm -rf /var/lib/apt/lists/*
RUN set -eu; \
    triple="$(uname -m)-unknown-linux-musl"; \
    rustup target add "$triple"; \
    printf '%s' "$triple" > /rust-target; \
    printf '[build]\ntarget = "%s"\n' "$triple" >> "$CARGO_HOME/config.toml"
ENV CARGO_INCREMENTAL=0 CARGO_TERM_COLOR=never CARGO_NET_RETRY=5
WORKDIR /src

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path /recipe.json

FROM chef AS deps
COPY --from=planner /recipe.json /recipe.json
# The recipe leaves out path dependencies outside the workspace: pacod-id's vendored crates.
COPY vendor/pacod-id/crates /src/vendor/pacod-id/crates
RUN --mount=type=cache,id=pacod-agg-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    cargo chef cook --release --locked --workspace --recipe-path /recipe.json

FROM deps AS build
COPY . .
RUN --mount=type=cache,id=pacod-agg-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    set -eu; \
    cargo build --release --locked --workspace --bin pacod-agg --bin pacod-agg-scraper --bin agg-mock; \
    for bin in pacod-agg pacod-agg-scraper agg-mock; do \
      install -D "target/$(cat /rust-target)/release/$bin" "/out/$bin"; \
    done

# The health probe: standard library only, for an image without curl.
FROM chef AS healthcheck
COPY deploy/docker/healthcheck.rs /src/healthcheck.rs
# rustc writes into the output's directory and does not create it. The musl target links
# statically by default.
RUN set -eu; \
    mkdir -p /out; \
    rustc --edition 2024 --target "$(cat /rust-target)" \
      -C opt-level=s -C panic=abort -C codegen-units=1 -C strip=symbols \
      -o /out/healthcheck /src/healthcheck.rs

FROM ${RUNTIME_IMAGE} AS runtime
COPY --from=healthcheck /out/healthcheck /usr/local/bin/healthcheck
USER 65532:65532
STOPSIGNAL SIGTERM

# server: the catalogue on 8150, the admin and internal API on 8151, metrics on 9150.
FROM runtime AS server
ARG VERSION=dev
ARG VCS_REF=unknown
LABEL org.opencontainers.image.title="pacod-agg" \
      org.opencontainers.image.description="pacod-aggregator: model data for AI gateways, read from the vendors' own pages" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${VCS_REF}"
COPY --from=build /out/pacod-agg /usr/local/bin/pacod-agg
EXPOSE 8150 8151 9150
ENTRYPOINT ["/usr/local/bin/pacod-agg"]

# scraper: agg-scraper. /healthz and /readyz on 8152; the egress proxy its browser goes out
# through on 3128.
FROM runtime AS scraper
ARG VERSION=dev
ARG VCS_REF=unknown
LABEL org.opencontainers.image.title="pacod-agg-scraper" \
      org.opencontainers.image.description="pacod-aggregator's scraper: renders the vendors' pages and looks for them" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${VCS_REF}"
COPY --from=build /out/pacod-agg-scraper /usr/local/bin/pacod-agg-scraper
EXPOSE 8152 3128
ENTRYPOINT ["/usr/local/bin/pacod-agg-scraper"]

# mock: agg-mock, a vendor's site and an extraction model. Development and tests only.
FROM runtime AS mock
COPY --from=build /out/agg-mock /usr/local/bin/agg-mock
EXPOSE 9100
ENTRYPOINT ["/usr/local/bin/agg-mock"]
