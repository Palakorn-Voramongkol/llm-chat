#requires -Version 5.1
<#
.SYNOPSIS
  Launch the native Windows worker for the llm-chat compose stack.
  Binds 0.0.0.0:7878 with the shared token so the manager container can
  reach it via host.docker.internal:7878. Start this BEFORE `docker compose up`
  (the manager's startup probe of :7878 is fatal — spec §7.1).
#>
[CmdletBinding()]
param(
    [string]$Token,
    [int]$Port = 7878,
    [string]$Bind = "0.0.0.0",
    [string]$EnvFile = (Join-Path $PSScriptRoot "..\..\.env"),
    [string]$WorkerExe
)

$ErrorActionPreference = "Stop"

function Read-DotEnvValue([string]$Path, [string]$Key) {
    if (-not (Test-Path $Path)) { return $null }
    foreach ($line in Get-Content -LiteralPath $Path) {
        $t = $line.Trim()
        if ($t -eq "" -or $t.StartsWith("#")) { continue }
        $eq = $t.IndexOf("=")
        if ($eq -lt 1) { continue }
        $k = $t.Substring(0, $eq).Trim()
        if ($k -eq $Key) {
            return $t.Substring($eq + 1).Trim().Trim('"')
        }
    }
    return $null
}

function Resolve-WorkerExe([string]$RepoRoot, [string]$Override) {
    if ($Override) { return $Override }
    $release = Join-Path $RepoRoot "worker\target\release\llm-chat.exe"
    $debug   = Join-Path $RepoRoot "worker\target\debug\llm-chat.exe"
    if (Test-Path $release) { return $release }
    if (Test-Path $debug)   { return $debug }
    throw "worker binary not found. Build it first (cargo build --release in worker/) or pass -WorkerExe."
}

function Invoke-RunWorker {
    param([string]$Token, [int]$Port, [string]$Bind, [string]$EnvFile, [string]$WorkerExe)

    if (-not $Token) { $Token = Read-DotEnvValue -Path $EnvFile -Key "LLM_CHAT_AUTH_TOKEN" }
    if (-not $Token) {
        throw "LLM_CHAT_AUTH_TOKEN not provided (-Token) and not found in $EnvFile. " +
              "Copy .env.example to .env and set it (openssl rand -hex 32)."
    }

    $repoRoot = Resolve-Path (Join-Path $PSScriptRoot "..\..")
    $exe = Resolve-WorkerExe -RepoRoot $repoRoot -Override $WorkerExe

    Write-Host "[run-worker] worker  = $exe"
    Write-Host "[run-worker] bind    = ${Bind}:${Port}"
    Write-Host "[run-worker] token   = (len=$($Token.Length))"
    Write-Host "[run-worker] NOTE: Windows Defender Firewall may prompt for the 0.0.0.0 bind."
    Write-Host "[run-worker]       Approve it (PRIVATE networks only) or the manager cannot reach :$Port."

    $env:LLM_CHAT_AUTH_TOKEN = $Token
    $env:LLM_CHAT_WS_PORT     = "$Port"
    $env:LLM_CHAT_WS_BIND     = $Bind

    # Foreground/blocking by design: holds the session for the GUI worker's
    # lifetime so Ctrl-C stops the worker.
    & $exe
}

# Only run the launch tail when executed directly, NOT when dot-sourced
# (`. .\run-worker.ps1`). Dot-sourcing exposes the functions for testing
# without launching the windowless GUI worker.
if ($MyInvocation.InvocationName -ne '.') {
    Invoke-RunWorker -Token $Token -Port $Port -Bind $Bind -EnvFile $EnvFile -WorkerExe $WorkerExe
}
