#!/bin/bash
set -e

ROOT_DIR=$(pwd)

# Ensure apps are built and fixtures are ready
echo "Building test applications..."
"$ROOT_DIR/scripts/build_apps.sh"

echo "Running exorun test suite..."
cd "$ROOT_DIR/crates/exorun"

# Run all tests with output visibility
cargo test -- --nocapture

echo "All exorun tests passed!"
