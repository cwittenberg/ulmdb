# Build for the selected Linux platform's portable Rust CPU baseline.
# No target-cpu=native, comparison adapters, fault injection or server is enabled.
ARG BUILD_IMAGE=docker.io/library/rust:1.94.0-slim-bookworm@sha256:a86cada82e36ebd7a9bffed7548792c55a952fdb20718eea9278a936bcb76e62
ARG RUNTIME_IMAGE=docker.io/library/debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3
FROM ${BUILD_IMAGE} AS build
ARG BUILD_IMAGE
WORKDIR /build/ulmdb
ENV RUSTUP_TOOLCHAIN=1.94.0 CARGO_INCREMENTAL=0 RUSTFLAGS="" \
    CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_STRIP=debuginfo
RUN printf '%s\n' "$BUILD_IMAGE" | grep -Eq '@sha256:[0-9a-f]{64}$' \
    && rustc --version | grep -Eq '^rustc 1\.94\.0 '
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY clients ./clients
RUN cargo build --release --locked --no-default-features -p ulmdb -p ulmdb-ipdon --bins

FROM ${RUNTIME_IMAGE} AS runtime
ARG RUNTIME_IMAGE
ARG SOURCE_REVISION=unrecorded
LABEL org.opencontainers.image.title="ULMDB command-line tools" \
    org.opencontainers.image.description="Experimental mapped database, file replication and frozen-data ipdon CLI" \
    org.opencontainers.image.version="0.1.0" \
    org.opencontainers.image.revision="$SOURCE_REVISION" \
    org.opencontainers.image.base.name="$RUNTIME_IMAGE"
RUN printf '%s\n' "$RUNTIME_IMAGE" | grep -Eq '@sha256:[0-9a-f]{64}$' \
    && install -d -o 10001 -g 10001 -m 0700 /data /staging /run/ulmdb-secrets
COPY --from=build /build/ulmdb/target/release/ulmdb /usr/local/bin/ulmdb
COPY --from=build /build/ulmdb/target/release/ulmdb-replicate /usr/local/bin/ulmdb-replicate
COPY --from=build /build/ulmdb/target/release/ipdon /usr/local/bin/ipdon
USER 10001:10001
WORKDIR /data
ENTRYPOINT ["/usr/local/bin/ulmdb"]
CMD ["stats", "/data/db"]
