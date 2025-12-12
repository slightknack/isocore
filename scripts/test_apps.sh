#!/bin/bash
set -e

ROOT_DIR=$(pwd)

# Ensure apps are built and fixtures are ready
"$ROOT_DIR/scripts/build_apps.sh"

echo "Running tests..."
cd "$ROOT_DIR/crates/isorun"
cargo test --test integration_suite -- --nocapture
