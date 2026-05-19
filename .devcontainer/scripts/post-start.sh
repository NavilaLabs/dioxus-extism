#!/bin/bash
set -e

sudo chown -R "$(whoami)":"$(whoami)" /workspaces/dioxus-extism

# Make scripts executable if any project scripts exist
if [ -d "/workspaces/dioxus-extism/scripts" ]; then
    chmod +x /workspaces/dioxus-extism/scripts/*.sh 2>/dev/null || true
fi

# Ensure Deno is in PATH
export PATH="$HOME/.deno/bin:$PATH"
if ! grep -q 'deno/bin' ~/.bashrc; then
    echo 'export PATH="$HOME/.deno/bin:$PATH"' >> ~/.bashrc
fi

# Reinstall global Deno packages if missing
if ! command -v tailwind >/dev/null 2>&1; then
    deno install -fg npm:tailwind npm:@tailwindcss/cli
fi

# Confirm toolchain is ready
echo "--- dioxus-extism devcontainer ready ---"
rustc --version
cargo --version
dx --version
echo "Targets: $(rustup target list --installed | tr '\n' ' ')"
echo "---------------------------------------"
