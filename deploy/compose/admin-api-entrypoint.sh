#!/bin/sh
# admin-api entrypoint: compose cannot env_file a path that only exists inside a
# runtime volume, so source the provisioner's generated env (project_id /
# audience) and resolve the OIDC client id/secret from the mounted secret files
# before exec-ing the binary. AdminConfig::from_env then sees every required var.
set -eu

# project_id + audience (ZITADEL_PROJECT_ID / ZITADEL_AUDIENCE), written by the
# provisioner into manager.generated.env on the genenv volume mounted at /out.
if [ -f /out/manager.generated.env ]; then
    # shellcheck disable=SC1091
    set -a; . /out/manager.generated.env; set +a
else
    echo "admin-api-entrypoint: /out/manager.generated.env missing (provisioner not done?)" >&2
    exit 1
fi

# OIDC client id/secret: read the file path indirection if the value is unset.
if [ -z "${ADMIN_OIDC_CLIENT_ID:-}" ] && [ -n "${ADMIN_OIDC_CLIENT_ID_FILE:-}" ]; then
    ADMIN_OIDC_CLIENT_ID="$(cat "$ADMIN_OIDC_CLIENT_ID_FILE")"; export ADMIN_OIDC_CLIENT_ID
fi
if [ -z "${ADMIN_OIDC_CLIENT_SECRET:-}" ] && [ -n "${ADMIN_OIDC_CLIENT_SECRET_FILE:-}" ]; then
    ADMIN_OIDC_CLIENT_SECRET="$(cat "$ADMIN_OIDC_CLIENT_SECRET_FILE")"; export ADMIN_OIDC_CLIENT_SECRET
fi

exec /usr/local/bin/llm-chat-admin-api
