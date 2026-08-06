#!/bin/bash
# Build and deploy to Raspberry Pi by compiling directly on the Pi
set -e

PI_HOST=${1:-raspberrypi}
PI_USER=${2:-pi}
PI_SRC_DIR=${3:-/tmp/edit_sys_time}
PI_BINARY=${4:-/usr/local/bin/edit_sys_time}

echo "=== Syncing source to $PI_USER@$PI_HOST ==="
rsync -avz --exclude target/ ./ "$PI_USER@$PI_HOST:$PI_SRC_DIR"

echo "=== Building on Raspberry Pi ==="
ssh "$PI_USER@$PI_HOST" "cd $PI_SRC_DIR && cargo build --release"

echo "=== Installing binary ==="
ssh "$PI_USER@$PI_HOST" "sudo cp $PI_SRC_DIR/target/release/edit_sys_time $PI_BINARY"

echo "=== Installing systemd service ==="
scp deploy/edit_sys_time.service "$PI_USER@$PI_HOST:/tmp/edit_sys_time.service"
ssh "$PI_USER@$PI_HOST" "\
    sudo mv /tmp/edit_sys_time.service /etc/systemd/system/edit_sys_time.service && \
    sudo systemctl daemon-reload && \
    sudo systemctl enable edit_sys_time.service && \
    sudo systemctl restart edit_sys_time.service"

echo "=== Done ==="
ssh "$PI_USER@$PI_HOST" "sudo systemctl status edit_sys_time.service --no-pager"
