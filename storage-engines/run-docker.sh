#!/usr/bin/env bash
# Run from any directory. Dependencies and PostgreSQL stay inside Docker.
set -euo pipefail
repo=$(cd "$(dirname "$0")/.." && pwd)
image=${ULMDB_SQL_IMAGE:-ulmdb-sql:local}
mkdir -p "$repo/target"
data=$(mktemp -d "$repo/target/sql-engines.XXXXXXXX")
# The container runs as its unprivileged postgres account. These newly created
# directories contain only this run's generated files.
chmod 0777 "$data"
docker build -f "$repo/storage-engines/Dockerfile" -t "$image" "$repo"
docker run --rm --network none --cap-drop ALL \
  --security-opt no-new-privileges --user postgres \
  --mount "type=bind,source=$data,target=/sql-data" \
  --entrypoint sh "$image" /src/storage-engines/test-container.sh "$@"
printf 'Data retained for inspection: %s\n' "$data"
