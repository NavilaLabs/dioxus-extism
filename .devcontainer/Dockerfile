FROM mcr.microsoft.com/devcontainers/base:ubuntu-24.04

# Install system dependencies
RUN apt-get update && apt-get install -y \
  build-essential \
  curl \
  wget \
  pkg-config

# Install rust-analyzer (latest release)
RUN curl -L https://github.com/rust-lang/rust-analyzer/releases/latest/download/rust-analyzer-$(uname -m)-unknown-linux-gnu.gz \
    | gunzip -c - > /usr/local/bin/rust-analyzer \
    && chmod +x /usr/local/bin/rust-analyzer

# Switch to non-root user
USER vscode
WORKDIR /home/vscode

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# Add Cargo to PATH
ENV PATH="/home/vscode/.cargo/bin:${PATH}"

# Finish Rust setup
RUN rustup toolchain install stable \
 && rustup target add wasm32-unknown-unknown

RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash

RUN cargo binstall cargo-tarpaulin
RUN cargo binstall prek --git https://github.com/j178/prek

RUN git remote add template git@github.com:NavilaLabs/rust-lib.git 2>/dev/null || true
