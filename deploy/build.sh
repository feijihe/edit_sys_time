#!/bin/bash
set -e

TARGET=${1:-armv7-unknown-linux-gnueabihf}
RELEASE=${2:-release}

echo "=== Building for $TARGET ($RELEASE) ==="

if [ "$RELEASE" = "release" ]; then
    cargo build --target "$TARGET" --release
    BINARY="target/$TARGET/release/edit_sys_time"
else
    cargo build --target "$TARGET"
    BINARY="target/$TARGET/debug/edit_sys_time"
fi

if [ -f "$BINARY" ]; then
    echo "=== Stripping binary ==="
    rust-objcopy --strip-all "$BINARY" "$BINARY.stripped" 2>/dev/null || \
        arm-linux-gnueabihf-strip "$BINARY" -o "$BINARY.stripped" 2>/dev/null || \
        cp "$BINARY" "$BINARY.stripped"

    echo "=== Binary ready: $BINARY.stripped ==="
    ls -lh "$BINARY.stripped"
else
    echo "ERROR: Binary not found at $BINARY"
    exit 1
fi
