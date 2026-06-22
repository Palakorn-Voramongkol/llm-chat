#!/usr/bin/env bash
# Destructively reset the llm-chat stack and reseed Zitadel from scratch.
#
# Wipes the pgdata / machinekey / genenv volumes AND the host ./secrets dir,
# rebuilds the seed image (zitadel-init) from the CURRENT provision.py, then
# runs `docker compose up -d`, which auto-reseeds a fresh Zitadel.
#
# EVERYTHING is regenerated: all users, chat history, usage stats, the bootstrap
# IAM_OWNER key, every service-account key, and the admin / chatter passwords.
#
# The `build zitadel-init` step is deliberate: `down -v` cannot touch ./secrets
# (a host bind mount), and `up` alone would reseed from a STALE image if
# provision.py changed -- so we clear the secrets and rebuild the image first.
#
# Usage: ./reset.sh [-f|--force]    (-f/--force skips the confirmation prompt)
set -euo pipefail
cd "$(dirname "$0")"

FORCE=0
for arg in "$@"; do
  case "$arg" in
    -f|--force) FORCE=1 ;;
    *) echo "unknown arg: $arg (usage: ./reset.sh [-f|--force])" >&2; exit 2 ;;
  esac
done

if [ "$FORCE" -ne 1 ]; then
  echo "This DESTROYS all Zitadel data + ./secrets and regenerates every password."
  read -r -p "Type 'reset' to confirm: " answer
  [ "$answer" = "reset" ] || { echo "Aborted."; exit 1; }
fi

echo "==> docker compose down -v"
docker compose down -v

echo "==> removing ./secrets (host bind mount; down -v cannot)"
rm -rf ./secrets

echo "==> rebuilding seed image from current provision.py"
docker compose build zitadel-init

echo "==> docker compose up -d (auto-reseeds Zitadel)"
docker compose up -d

# `up -d` blocks until zitadel-init completes (services depend on it via
# service_completed_successfully), so the regenerated secrets exist by now.
echo "Done. Fresh credentials:"
if [ -f ./secrets/admin_password ]; then
  echo "  admin    : $(cat ./secrets/admin_user) / $(cat ./secrets/admin_password)"
  echo "  chatter  : $(cat ./secrets/chatter_user) / $(cat ./secrets/chatter_password)"
  echo "  Console  : http://localhost:3000"
else
  echo "  (secrets not found - check 'docker compose logs zitadel-init')"
fi
