#!/bin/bash
set -e

ROOT_DIR=$(pwd)
FIXTURES_DIR="$ROOT_DIR/crates/isorun/tests/fixtures"

mkdir -p "$FIXTURES_DIR"

build_app() {
    APP_NAME=$1
    echo "Building $APP_NAME..."

    cd "$ROOT_DIR/apps/$APP_NAME"

    # Build directly with wasm32-wasip2 target (native component support, no adapter needed)
    cargo build --release --target wasm32-wasip2

    # Copy the component to fixtures
    cp "target/wasm32-wasip2/release/$APP_NAME.wasm" "$FIXTURES_DIR/$APP_NAME.wasm"
}

# Discover and build all apps in the apps/ directory
for app_dir in "$ROOT_DIR/apps"/*/ ; do
    if [ -d "$app_dir" ]; then
        app_name=$(basename "$app_dir")
        build_app "$app_name"
    fi
done
