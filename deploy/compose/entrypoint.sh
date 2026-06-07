#!/bin/sh
set -e
set -a
. /out/manager.generated.env
set +a
: "${ZITADEL_ISSUER:?ZITADEL_ISSUER missing — refusing to start in shared-token mode}"
: "${ZITADEL_PROJECT_ID:?ZITADEL_PROJECT_ID missing from manager.generated.env}"
: "${ZITADEL_AUDIENCE:?ZITADEL_AUDIENCE missing from manager.generated.env}"
exec /usr/local/bin/llm-chat-manager
