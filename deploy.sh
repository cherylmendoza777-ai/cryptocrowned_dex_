#!/bin/bash

echo "🚀 Deploying update..."

cd /root/cryptocrowned_dex_

git pull origin main

# Build backend
cd cryptocrowned_dex
cargo build --release

# Restart backend
systemctl restart cryptocrowned-dex

# Build frontend
cd ../frontend-react
"C:\Program Files\nodejs\npm.cmd" install
"C:\Program Files\nodejs\npm.cmd" run build

echo "✅ Deployment complete"
