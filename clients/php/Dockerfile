# Build the native engine and run PHP entirely inside containers.
ARG BUILD_IMAGE=docker.io/library/rust:1.94.0-slim-bookworm@sha256:a86cada82e36ebd7a9bffed7548792c55a952fdb20718eea9278a936bcb76e62
ARG PHP_IMAGE=docker.io/library/php:8.3-cli-bookworm
FROM ${BUILD_IMAGE} AS engine
WORKDIR /build/ulmdb
ENV RUSTUP_TOOLCHAIN=1.94.0 CARGO_INCREMENTAL=0 RUSTFLAGS=""
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY clients/c ./clients/c
COPY clients/rust ./clients/rust
# Cargo resolves every workspace member, even for this selected package.
COPY utility/explorer/Cargo.toml ./utility/explorer/Cargo.toml
COPY utility/explorer/src ./utility/explorer/src
RUN cargo build --locked -p ulmdb-c

FROM ${PHP_IMAGE} AS test
RUN apt-get update \
    && apt-get install -y --no-install-recommends libffi-dev python3 \
    && docker-php-ext-install -j2 ffi \
    && rm -rf /var/lib/apt/lists/* \
    && printf 'ffi.enable=true\n' > /usr/local/etc/php/conf.d/ulmdb.ini
WORKDIR /work/ulmdb
COPY --from=engine /build/ulmdb/target/debug/libulmdb_c.so /usr/local/lib/libulmdb_c.so
COPY clients/php ./clients/php
COPY clients/python/ulmdb ./clients/python/ulmdb
ENV ULMDB_LIBRARY=/usr/local/lib/libulmdb_c.so \
    PYTHONPATH=/work/ulmdb/clients/python PYTHONDONTWRITEBYTECODE=1
USER 10001:10001
CMD ["sh", "-ec", "php --version; for source in clients/php/autoload.php clients/php/src/*.php clients/php/tests/*.php clients/php/examples/*.php; do php -l \"$source\"; done; php clients/php/tests/client.php; python3 clients/php/tests/interoperability.py; php clients/php/examples/basic.php"]
