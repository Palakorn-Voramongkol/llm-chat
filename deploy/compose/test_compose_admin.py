"""Contract tests for the admin-api / admin-web compose services.

Parses docker-compose.yml as data (PyYAML) and .env.example as text. No Docker
daemon — the real stack boot is the Task 26 acceptance task.
"""
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]
COMPOSE = ROOT / "docker-compose.yml"
ENV_EXAMPLE = ROOT / ".env.example"
ENTRYPOINT = ROOT / "deploy" / "compose" / "admin-api-entrypoint.sh"


def _services() -> dict:
    return yaml.safe_load(COMPOSE.read_text(encoding="utf-8"))["services"]


def test_admin_api_service() -> None:
    svc = _services()["admin-api"]
    # built from the Phase-E Dockerfile, root build context (workspace build)
    assert svc["build"]["context"] == "."
    assert svc["build"]["dockerfile"] == "deploy/compose/admin-api.Dockerfile"
    assert svc["ports"] == ["7676:7676"]
    # waits for the one-shot provisioner to finish (SA key + OIDC secret + pid)
    assert svc["depends_on"]["zitadel-init"]["condition"] == "service_completed_successfully"
    env = svc["environment"]
    assert env["ADMIN_BIND_ADDR"] == "0.0.0.0:7676"
    assert env["ZITADEL_ISSUER"] == "http://host.docker.internal:8080"
    assert env["ADMIN_SA_KEY_PATH"] == "/secrets/admin-api-key.json"
    assert env["ADMIN_SESSION_KEY"] == "${ADMIN_SESSION_KEY}"
    assert env["ADMIN_PUBLIC_ORIGIN"] == "http://localhost:7676"
    assert env["ADMIN_ALLOWED_ORIGIN"] == "http://localhost:3000"
    # generated env (project_id / audience) + the secrets dir are mounted in
    mounts = svc["volumes"]
    assert "genenv:/out:ro" in mounts
    assert "./secrets:/secrets:ro" in mounts


def test_admin_web_service() -> None:
    svc = _services()["admin-web"]
    assert svc["build"]["context"] == "./admin-web"
    assert svc["build"]["dockerfile"] == "../deploy/compose/admin-web.Dockerfile"
    assert svc["ports"] == ["3000:3000"]
    # web is a pure client of admin-api; proxy target is the in-network host
    assert svc["depends_on"]["admin-api"]["condition"] == "service_started"
    assert svc["environment"]["ADMIN_API_ORIGIN"] == "http://admin-api:7676"


def test_entrypoint_sources_generated_env() -> None:
    t = ENTRYPOINT.read_text(encoding="utf-8")
    # sources project_id/audience from the generated env mounted at /out
    assert "/out/manager.generated.env" in t
    # resolves the OIDC client id/secret from the *_FILE indirection
    assert "ADMIN_OIDC_CLIENT_ID_FILE" in t
    assert "ADMIN_OIDC_CLIENT_SECRET_FILE" in t
    assert "exec /usr/local/bin/llm-chat-admin-api" in t


def test_env_example_has_session_key() -> None:
    t = ENV_EXAMPLE.read_text(encoding="utf-8")
    assert "ADMIN_SESSION_KEY=" in t
