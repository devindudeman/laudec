#!/usr/bin/env bash
# Install laudec — see everything Claude Code does.
# Usage: curl -fsSL https://raw.githubusercontent.com/devindudeman/laudec/main/install.sh | bash
set -euo pipefail

REPO="https://github.com/devindudeman/laudec.git"
INSTALL_DIR="${LAUDEC_INSTALL_DIR:-$HOME/.local/bin}"

info()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
err()   { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

# ── Check prerequisites ──────────────────────────────────────────────
command -v git   >/dev/null 2>&1 || err "git is required"
command -v cargo >/dev/null 2>&1 || err "Rust toolchain is required (https://rustup.rs)"
command -v node  >/dev/null 2>&1 || err "Node.js is required (https://nodejs.org)"
command -v npm   >/dev/null 2>&1 || err "npm is required"

# ── Clone ─────────────────────────────────────────────────────────────
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

info "Cloning laudec..."
git clone --depth 1 "$REPO" "$TMPDIR/laudec" 2>&1 | tail -1
cd "$TMPDIR/laudec"

# ── Build dashboard ──────────────────────────────────────────────────
info "Building dashboard..."
cd dashboard
npm install --silent 2>&1 | tail -1
npm run build 2>&1 | tail -1
cd ..

# ── Build binary ─────────────────────────────────────────────────────
info "Building laudec (this may take a minute)..."
cargo build --release 2>&1 | tail -3

# ── Install ──────────────────────────────────────────────────────────
mkdir -p "$INSTALL_DIR"
cp target/release/laudec "$INSTALL_DIR/laudec"
chmod +x "$INSTALL_DIR/laudec"

info "Installed to $INSTALL_DIR/laudec"

# ── Check PATH ───────────────────────────────────────────────────────
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
    echo ""
    echo "  Add to your PATH by adding this to your shell profile:"
    echo ""
    case "${SHELL:-/bin/bash}" in
        */zsh)  echo "    echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.zshrc" ;;
        */fish) echo "    fish_add_path $INSTALL_DIR" ;;
        *)      echo "    echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.bashrc" ;;
    esac
    echo ""
    echo "  Then restart your shell or run: export PATH=\"$INSTALL_DIR:\$PATH\""
else
    echo ""
    echo "  Run 'laudec .' in any project directory to get started."
fi

echo ""
