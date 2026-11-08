#!/usr/bin/env bash
# No host PHP, Composer, Rust build, or bind-mounted writable database required.
set -euo pipefail
cd "$(dirname "$0")/../.."
image="${ULMDB_PHP_TEST_IMAGE:-ulmdb-php-test:local}"
php_image="${PHP_IMAGE:-docker.io/library/php:8.3-cli-bookworm}"
docker build --build-arg "PHP_IMAGE=$php_image" -f clients/php/Dockerfile -t "$image" .
docker run --rm --network none --cap-drop ALL --security-opt no-new-privileges "$image"
