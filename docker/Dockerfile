# Dockerfile for probe-verus
# Runs `probe-verus atomize` and `probe-verus verify` on a project
#
# Build:
#   docker build -t probe-verus -f docker/Dockerfile .
#
# Usage:
#   docker run -v /path/to/project:/workspace/project -v /path/to/output:/workspace/output probe-verus project
#
# Output:
#   - /workspace/output/atoms.json (from atomize)
#   - /workspace/output/results.json (from verify)

FROM ubuntu:22.04

# Version argument for base Rust toolchain (for building probe-verus)
ARG RUST_VERSION=1.88.0

# Set environment variables
ENV DEBIAN_FRONTEND=noninteractive
ENV CARGO_TERM_COLOR=always

# Install system dependencies
RUN apt-get update && apt-get install -y \
    curl \
    wget \
    unzip \
    jq \
    python3 \
    build-essential \
    gcc \
    libc6-dev \
    pkg-config \
    protobuf-compiler \
    cmake \
    clang \
    libclang-dev \
    libssl-dev \
    git \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Install Rust toolchain
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain "${RUST_VERSION}"
ENV PATH="/root/.cargo/bin:${PATH}"

# Verify Rust installation
RUN rustc --version && cargo --version

# ============================================================================
# Install Verus (latest stable release)
# ============================================================================
# Download Verus, then install the Rust toolchain it requires
RUN echo "Installing Verus..." && \
    RELEASE_JSON=$(curl -s https://api.github.com/repos/verus-lang/verus/releases) && \
    RELEASE=$(echo "$RELEASE_JSON" | jq -r '.[] | select(.prerelease == false) | .tag_name' | head -n1) && \
    echo "Installing Verus release: $RELEASE" && \
    RELEASE_DATA=$(echo "$RELEASE_JSON" | jq -r --arg release "$RELEASE" '.[] | select(.tag_name == $release)') && \
    DOWNLOAD_URL=$(echo "$RELEASE_DATA" | jq -r '.assets[] | select(.name | contains("x86-linux.zip")) | .browser_download_url') && \
    echo "Download URL: $DOWNLOAD_URL" && \
    wget -q "$DOWNLOAD_URL" -O verus.zip && \
    unzip -q verus.zip && \
    mv verus-x86-linux /opt/verus && \
    ln -s /opt/verus/verus /usr/local/bin/verus && \
    ln -s /opt/verus/cargo-verus /usr/local/bin/cargo-verus && \
    rm -f verus.zip && \
    echo "Verus installed successfully"

# Install the Rust toolchain that Verus requires (detected dynamically)
RUN VERUS_OUTPUT=$(verus --version 2>&1 || true) && \
    echo "Verus output: $VERUS_OUTPUT" && \
    NEEDED_RUST=$(echo "$VERUS_OUTPUT" | grep -oP 'toolchain \K[0-9]+\.[0-9]+\.[0-9]+-[a-z0-9_-]+' | head -1) && \
    if [ -n "$NEEDED_RUST" ]; then \
        echo "Installing Rust toolchain: $NEEDED_RUST" && \
        rustup install "$NEEDED_RUST"; \
    fi && \
    verus --version

# ============================================================================
# Install verus-analyzer (required for atomize command)
# ============================================================================
# Use direct download URL (more reliable than GitHub API)
RUN echo "Installing verus-analyzer..." && \
    wget -q "https://github.com/verus-lang/verus-analyzer/releases/latest/download/verus-analyzer-x86_64-unknown-linux-gnu.gz" -O verus-analyzer.gz && \
    gunzip verus-analyzer.gz && \
    chmod +x verus-analyzer && \
    mv verus-analyzer /usr/local/bin/ && \
    verus-analyzer --version

# ============================================================================
# Install scip CLI (required for atomize command)
# ============================================================================
# Use direct download URL (more reliable than GitHub API)
RUN echo "Installing scip..." && \
    wget -q "https://github.com/sourcegraph/scip/releases/latest/download/scip-linux-amd64.tar.gz" -O scip.tar.gz && \
    tar -xzf scip.tar.gz && \
    chmod +x scip && \
    mv scip /usr/local/bin/ && \
    rm -f scip.tar.gz && \
    scip --version || scip --help | head -5

# ============================================================================
# Build and install probe-verus
# ============================================================================
WORKDIR /build

# Copy probe-verus source
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/

# Build probe-verus in release mode
RUN cargo build --release && \
    cp target/release/probe-verus /usr/local/bin/ && \
    rm -rf /build

# Verify probe-verus installation
RUN probe-verus --help

# ============================================================================
# Create non-root user for security
# ============================================================================
ARG USER_UID=1000
ARG USER_GID=1000

RUN groupadd --gid "${USER_GID}" verus \
    && useradd --uid "${USER_UID}" --gid "${USER_GID}" -m verus \
    && mkdir -p /workspace/output \
    && chown -R verus:verus /workspace \
    && chmod -R 755 /opt/verus

# Make Rust toolchain accessible to non-root users (read-only)
RUN chmod -R a+rX /root/.rustup /root/.cargo

# Set PATH to include all tool locations
ENV PATH="/usr/local/bin:/root/.cargo/bin:${PATH}"

# ============================================================================
# Set up workspace
# ============================================================================
WORKDIR /workspace

# Run as non-root user by default (security best practice)
# Override with --user root if needed for cargo operations
USER verus

# Default entrypoint: probe-verus run command (no shell script needed)
ENTRYPOINT ["probe-verus", "run"]

# Default command shows help
CMD ["--help"]

