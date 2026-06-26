#!/usr/bin/env pwsh
<#
.SYNOPSIS
  Destructively reset the llm-chat stack and reseed Zitadel from scratch.

.DESCRIPTION
  Wipes the pgdata / machinekey / genenv volumes AND the host ./secrets dir,
  rebuilds the seed image (zitadel-init) from the CURRENT provision.py, then
  runs `docker compose up -d`, which auto-reseeds a fresh Zitadel.

  EVERYTHING is regenerated: all users, chat history, usage stats, the bootstrap
  IAM_OWNER key, every service-account key, and the admin / chatter passwords.

  The `build zitadel-init` step is deliberate: `down -v` cannot touch ./secrets
  (a host bind mount), and `up` alone would reseed from a STALE image if
  provision.py changed — so we clear the secrets and rebuild the image first.

.PARAMETER Force
  Skip the typed confirmation prompt (for automation / CI).

.EXAMPLE
  ./reset.ps1
.EXAMPLE
  ./reset.ps1 -Force
#>
[CmdletBinding()]
param([switch]$Force)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

if (-not $Force) {
    Write-Host "This DESTROYS all Zitadel data + ./secrets and regenerates every password." -ForegroundColor Yellow
    $answer = Read-Host "Type 'reset' to confirm"
    if ($answer -ne 'reset') { Write-Host "Aborted."; exit 1 }
}

Write-Host "==> docker compose down -v" -ForegroundColor Cyan
docker compose down -v

Write-Host "==> removing ./secrets (host bind mount; down -v cannot)" -ForegroundColor Cyan
if (Test-Path .\secrets) { Remove-Item -Recurse -Force .\secrets }

Write-Host "==> rebuilding seed image from current provision.py" -ForegroundColor Cyan
docker compose build zitadel-init

# .env.local must exist BEFORE the services start (they load it via env_file).
# Regenerate it from the committed template; the two client-only IDs in it
# (PROJECT_ID/OIDC_CLIENT_ID) are filled in after the seed. No service uses
# those two, so the static service keys are all present from the start.
Write-Host "==> regenerating .env.local from .env.local.example" -ForegroundColor Cyan
Copy-Item .\.env.local.example .\.env.local -Force

Write-Host "==> docker compose up -d (auto-reseeds Zitadel)" -ForegroundColor Cyan
docker compose up -d

# `up -d` blocks until zitadel-init completes (services depend on it via
# service_completed_successfully), so the regenerated secrets exist by now.
# Sync the client-only IDs from the fresh seed so the host CLIs run flagless.
Write-Host "==> syncing .env.local PROJECT_ID/OIDC_CLIENT_ID from the fresh seed" -ForegroundColor Cyan
if ((Test-Path .\secrets\project_id) -and (Test-Path .\secrets\oidc_client_id)) {
    $projectId = (Get-Content .\secrets\project_id -Raw).Trim()
    $clientId = (Get-Content .\secrets\oidc_client_id -Raw).Trim()
    $envLocal = Get-Content .\.env.local -Raw
    $envLocal = $envLocal -replace '(?m)^PROJECT_ID=.*', "PROJECT_ID=$projectId"
    $envLocal = $envLocal -replace '(?m)^OIDC_CLIENT_ID=.*', "OIDC_CLIENT_ID=$clientId"
    Set-Content .\.env.local -Value $envLocal -NoNewline
}

Write-Host "Done. Fresh credentials:" -ForegroundColor Green
if (Test-Path .\secrets\admin_password) {
    "  admin    : {0} / {1}" -f (Get-Content .\secrets\admin_user -Raw).Trim(), (Get-Content .\secrets\admin_password -Raw).Trim()
    "  chatter  : {0} / {1}" -f (Get-Content .\secrets\chatter_user -Raw).Trim(), (Get-Content .\secrets\chatter_password -Raw).Trim()
    "  Console  : http://localhost:3000"
} else {
    Write-Host "  (secrets not found - check 'docker compose logs zitadel-init')" -ForegroundColor Yellow
}
