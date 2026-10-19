#!/bin/sh
# Installs the latest `fabric` release for macOS or Linux into ~/.local/bin (or $FABRIC_INSTALL_DIR).
#
#   curl -fsSL https://raw.githubusercontent.com/Project-Fabric/fabric/main/install.sh | sh
#
# Set FABRIC_VERSION=v0.1.0 to pick a version. The archive's SHA-256 checksum is verified
# before anything is installed.
set -eu

repo="Project-Fabric/fabric"
dir="${FABRIC_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target="aarch64-apple-darwin" ;;
  Darwin-x86_64) target="x86_64-apple-darwin" ;;
  Linux-x86_64) target="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64 | Linux-arm64) target="aarch64-unknown-linux-gnu" ;;
  *) echo "fabric: no prebuilt binary for $(uname -s) $(uname -m); build from source with cargo" >&2; exit 1 ;;
esac

version="${FABRIC_VERSION:-}"
if [ -z "$version" ]; then
  version=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
  [ -n "$version" ] || { echo "fabric: could not find the latest release" >&2; exit 1; }
fi

name="fabric-$version-$target"
url="https://github.com/$repo/releases/download/$version/$name.tar.gz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Downloading fabric $version for $target"
curl -fsSL "$url" -o "$tmp/$name.tar.gz"
curl -fsSL "$url.sha256" -o "$tmp/$name.tar.gz.sha256"
(cd "$tmp" && if command -v sha256sum >/dev/null; then sha256sum -c "$name.tar.gz.sha256"; else shasum -a 256 -c "$name.tar.gz.sha256"; fi) >/dev/null \
  || { echo "fabric: checksum verification failed; nothing was installed" >&2; exit 1; }

tar xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$dir"
install -m 755 "$tmp/$name/fabric" "$dir/fabric"
echo "Installed $dir/fabric"

case ":$PATH:" in
  *":$dir:"*) ;;
  *) echo "Add $dir to your PATH, e.g.: echo 'export PATH=\"$dir:\$PATH\"' >> ~/.zshrc" ;;
esac
echo "Next: cd into a project and run 'fabric init'"
