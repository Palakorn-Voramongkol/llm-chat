#!/usr/bin/env bash
# Install Zitadel as a native systemd service. Idempotent.
#
# Usage:
#   sudo ZITADEL_VERSION=v2.71.10 ./install.sh
#
# Pin a specific version with ZITADEL_VERSION; defaults to "latest".
# Run this as root (or with sudo). It will:
#   - download the Zitadel binary to /usr/local/bin/zitadel
#   - create the system user `zitadel` and group
#   - create /etc/zitadel and /var/lib/zitadel
#   - install the systemd unit
# It does NOT start the service — you fill in /etc/zitadel/zitadel.env first
# (copy from zitadel.env.example).

set -euo pipefail

VERSION="${ZITADEL_VERSION:-latest}"
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64)  ZARCH=amd64 ;;
    aarch64) ZARCH=arm64 ;;
    *) echo "unsupported arch: $ARCH" >&2; exit 1 ;;
esac

if [[ "$VERSION" == "latest" ]]; then
    URL="https://github.com/zitadel/zitadel/releases/latest/download/zitadel-linux-${ZARCH}.tar.gz"
else
    URL="https://github.com/zitadel/zitadel/releases/download/${VERSION}/zitadel-linux-${ZARCH}.tar.gz"
fi

echo "==> downloading $URL"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fL -o "$TMP/zitadel.tar.gz" "$URL"
tar -xzf "$TMP/zitadel.tar.gz" -C "$TMP"
# the archive contains a directory like zitadel-linux-amd64/zitadel
BIN="$(find "$TMP" -maxdepth 3 -type f -name zitadel | head -1)"
[[ -n "$BIN" ]] || { echo "binary not found in archive" >&2; exit 1; }

echo "==> installing /usr/local/bin/zitadel"
install -m 0755 "$BIN" /usr/local/bin/zitadel

echo "==> ensuring zitadel user/group"
getent group  zitadel >/dev/null || groupadd --system zitadel
getent passwd zitadel >/dev/null || useradd  --system --gid zitadel \
        --home-dir /var/lib/zitadel --shell /usr/sbin/nologin zitadel

echo "==> ensuring /etc/zitadel and /var/lib/zitadel"
install -d -m 0750 -o zitadel -g zitadel /etc/zitadel
install -d -m 0750 -o zitadel -g zitadel /var/lib/zitadel

# Drop the example env if no real one is in place yet.
if [[ ! -f /etc/zitadel/zitadel.env ]]; then
    install -m 0600 -o zitadel -g zitadel \
        "$(dirname "$0")/zitadel.env.example" \
        /etc/zitadel/zitadel.env
    echo "==> placed /etc/zitadel/zitadel.env (template — fill in the blanks)"
fi

echo "==> installing systemd unit"
install -m 0644 "$(dirname "$0")/zitadel.service" /etc/systemd/system/zitadel.service
systemctl daemon-reload

cat <<NEXT

Done. Next:

  1. sudo \$EDITOR /etc/zitadel/zitadel.env       # fill in the blanks
  2. sudo systemctl enable --now zitadel
  3. sudo journalctl -fu zitadel                  # watch first boot

Then visit https://id.palakorn.com/ui/console and log in with the
ZITADEL_FIRSTINSTANCE_* credentials you set in zitadel.env.
NEXT
