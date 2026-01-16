#!/usr/bin/env bash
# Run probe-verus Docker container on a project
#
# Usage:
#   ./docker/run.sh /path/to/project [OPTIONS]
#   ./docker/run.sh /path/to/project --atomize-only
#   ./docker/run.sh /path/to/project --package my-crate
#
# Output will be written to ./output/ in the current directory
#
# Environment variables:
#   PROBE_VERUS_IMAGE - Docker image to use (default: ghcr.io/beneficial-ai-foundation/probe-verus:latest)
#                       Set to "probe-verus" to use locally built image

set -eo pipefail

# Default to GHCR image, can override with env var
IMAGE="${PROBE_VERUS_IMAGE:-ghcr.io/beneficial-ai-foundation/probe-verus:latest}"

if [[ $# -lt 1 ]] || [[ "$1" == "--help" ]] || [[ "$1" == "-h" ]]; then
    echo "Usage: $0 /path/to/project [OPTIONS]"
    echo ""
    echo "Options (passed to probe-verus run):"
    echo "  --atomize-only      Run only atomize"
    echo "  --verify-only       Run only verify"
    echo "  --package <name>    Package name for workspaces"
    echo "  --regenerate-scip   Force SCIP regeneration"
    echo "  -v, --verbose       Verbose output"
    echo ""
    echo "Local options:"
    echo "  --output <dir>      Output directory (default: ./output)"
    echo ""
    echo "Environment variables:"
    echo "  PROBE_VERUS_IMAGE   Docker image (default: ghcr.io/beneficial-ai-foundation/probe-verus:latest)"
    echo "                      Use 'probe-verus' for locally built image"
    echo ""
    echo "Example:"
    echo "  $0 ~/my-verus-project"
    echo "  $0 ~/my-workspace --package my-crate"
    echo "  PROBE_VERUS_IMAGE=probe-verus $0 ~/my-project  # use local image"
    exit 0
fi

# Parse the project path (first argument)
PROJECT_PATH="$1"
shift

# Get absolute path
PROJECT_PATH="$(cd "$PROJECT_PATH" && pwd)"
PROJECT_NAME="$(basename "$PROJECT_PATH")"

# Default output directory
OUTPUT_DIR="./output"

# Parse arguments: separate local options from docker pass-through
DOCKER_ARGS=()
while [[ $# -gt 0 ]]; do
    case $1 in
        --output|-o)
            OUTPUT_DIR="$2"
            shift 2
            ;;
        *)
            DOCKER_ARGS+=("$1")
            shift
            ;;
    esac
done

# Create output directory
mkdir -p "$OUTPUT_DIR"
OUTPUT_DIR="$(cd "$OUTPUT_DIR" && pwd)"

echo "Project:  $PROJECT_PATH"
echo "Output:   $OUTPUT_DIR"
echo "Image:    $IMAGE"
echo ""

# Run the container
# Note: Project needs read-write access for Verus to build/verify
# Run as root to avoid permission issues with mounted volumes
docker run --rm \
    --user root \
    -v "$PROJECT_PATH:/workspace/project" \
    -v "$OUTPUT_DIR:/workspace/output" \
    "$IMAGE" \
    /workspace/project \
    --output /workspace/output \
    "${DOCKER_ARGS[@]}"
