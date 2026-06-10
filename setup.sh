#!/usr/bin/env bash
# ssharden setup — installs the bits needed to build and run the Tauri GUI.
#
# Rust (cargo) and the Bitwarden CLI (bw) are assumed already installed. This
# script focuses on the Tauri v2 Linux system libraries (the webkit stack), the
# Tauri CLI, and the frontend dependencies.
set -euo pipefail

echo "==> ssharden setup"

# --- 1. Toolchain sanity ---------------------------------------------------
command -v cargo >/dev/null 2>&1 || {
  echo "ERROR: cargo not found. Install Rust first: https://rustup.rs" >&2
  exit 1
}
command -v bun >/dev/null 2>&1 || {
  echo "ERROR: bun not found. Install it: https://bun.sh" >&2
  exit 1
}
command -v bw >/dev/null 2>&1 || {
  echo "WARN: bw (Bitwarden CLI) not found. Install with: npm install -g @bitwarden/cli" >&2
}

# --- 2. Tauri v2 Linux system libraries ------------------------------------
# These are required to compile the GUI shell (src-tauri). They are the one
# part of the build that needs root.
APT_PKGS=(
  libwebkit2gtk-4.1-dev
  build-essential
  curl
  wget
  file
  libxdo-dev
  libssl-dev
  libayatana-appindicator3-dev
  librsvg2-dev
  pkg-config
)

if command -v apt-get >/dev/null 2>&1; then
  if pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    echo "==> webkit2gtk-4.1 already present, skipping apt install"
  else
    echo "==> Installing Tauri system libraries via apt:"
    printf '      %s\n' "${APT_PKGS[@]}"
    if [ "$(id -u)" -eq 0 ]; then
      apt-get update && apt-get install -y "${APT_PKGS[@]}"
    elif command -v sudo >/dev/null 2>&1; then
      sudo apt-get update && sudo apt-get install -y "${APT_PKGS[@]}"
    else
      echo "ERROR: need root to apt-install. Re-run as root or install sudo, then:" >&2
      echo "  sudo apt-get install -y ${APT_PKGS[*]}" >&2
      exit 1
    fi
  fi
else
  echo "WARN: apt-get not found. Install the Tauri prerequisites for your distro:"
  echo "  https://tauri.app/start/prerequisites/"
fi

# --- 3. Tauri CLI ----------------------------------------------------------
if ! cargo tauri --version >/dev/null 2>&1; then
  echo "==> Installing the Tauri CLI (cargo install tauri-cli)"
  cargo install tauri-cli --locked
else
  echo "==> Tauri CLI already installed"
fi

# --- 4. Frontend deps ------------------------------------------------------
echo "==> Installing frontend dependencies (bun install)"
bun install

# --- 5. Next steps ---------------------------------------------------------
cat <<'EOF'

==> Done.

Next:
  1. Log in to your Vaultwarden once (serve only unlocks, it does not log in):
       bw config server https://your-vaultwarden.example.com
       bw login
  2. Run the app in dev mode:
       cargo tauri dev
  3. Run the core test suite anytime:
       cargo test -p ssharden-core
EOF
