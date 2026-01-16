#!/usr/bin/env bash
# Build the probe-verus Docker image
#
# Usage:
#   ./docker/build.sh
#   ./docker/build.sh --no-cache

set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

echo "Building probe-verus Docker image..."
echo "Project directory: $PROJECT_DIR"
echo ""

docker build \
    -t probe-verus \
    -f docker/Dockerfile \
    "$@" \
    .

echo ""
echo "✓ Build complete!"
echo ""
echo "Run with:"
echo "  docker run -v /path/to/project:/workspace/project -v /path/to/output:/workspace/output probe-verus project"

