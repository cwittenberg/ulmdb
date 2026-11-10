# Optional SQL adapters only. No changes to the engine are needed.
ARG RUST_IMAGE=rust:1.94.0-slim-bookworm
ARG POSTGRES_IMAGE=postgres:17-bookworm
FROM ${POSTGRES_IMAGE} AS toolchain
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       gcc make libc6-dev libsqlite3-dev sqlite3 python3 python3-psycopg2 \
       postgresql-server-dev-17 \
    && rm -rf /var/lib/apt/lists/*

FROM ${RUST_IMAGE} AS engine
WORKDIR /src
ENV RUSTUP_TOOLCHAIN=1.94.0 CARGO_INCREMENTAL=0
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY clients/c ./clients/c
COPY clients/rust ./clients/rust
COPY utility/explorer ./utility/explorer
COPY storage-engines/postgresql/native ./storage-engines/postgresql/native
RUN cargo build --release --locked -p ulmdb-c --features sql-batch

FROM engine AS sqlite-owned
COPY storage-engines/sqlite/native ./storage-engines/sqlite/native
RUN --mount=type=cache,id=ulmdb-sqlite-native-rust194,target=/native-target,sharing=locked \
    cargo build --release --locked --features versioned \
    --manifest-path storage-engines/sqlite/native/Cargo.toml \
    --target-dir /native-target \
    && mkdir -p /src/target/sqlite-native/release \
    && cp /native-target/release/libulmdb_sqlite_native.a /src/target/sqlite-native/release/

FROM toolchain AS adapters
WORKDIR /src
COPY clients/c/include ./clients/c/include
COPY storage-engines ./storage-engines
COPY --from=engine /src/target/release/libulmdb_c.so /usr/local/lib/libulmdb_c.so
COPY --from=sqlite-owned /src/target/sqlite-native/release/libulmdb_sqlite_native.a /usr/local/lib/libulmdb_sqlite_native.a
RUN ldconfig \
    && make -C storage-engines/sqlite all test_commit_failure.so ULMDB_LIBDIR=/usr/local/lib \
    && make -C storage-engines/sqlite owned ULMDB_NATIVE_LIB=/usr/local/lib/libulmdb_sqlite_native.a \
    && make -C storage-engines/postgresql ULMDB_LIBDIR=/usr/local/lib ULMDB_SQL_BATCH=1 \
    && make -C storage-engines/postgresql ULMDB_LIBDIR=/usr/local/lib ULMDB_SQL_BATCH=1 install
