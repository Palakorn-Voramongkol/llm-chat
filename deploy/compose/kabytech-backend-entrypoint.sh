#!/bin/sh
set -eu
# project_id + audience written by the provisioner into the shared /out volume.
. /out/manager.generated.env
export ZITADEL_PROJECT_ID ZITADEL_AUDIENCE
# OIDC client id/secret come from mounted secret files (never baked into the image).
KABY_OIDC_CLIENT_ID="$(cat /secrets/kabytech_oidc_client_id)"
KABY_OIDC_CLIENT_SECRET="$(cat /secrets/kabytech_oidc_client_secret)"
export KABY_OIDC_CLIENT_ID KABY_OIDC_CLIENT_SECRET
exec /usr/local/bin/kabytech-backend
