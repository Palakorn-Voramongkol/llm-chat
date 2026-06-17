# syntax=docker/dockerfile:1
FROM python:3-slim

# pyjwt[crypto] = pyjwt + cryptography (required RS256 backend, see spec §4.3).
RUN pip install --no-cache-dir "pyjwt[crypto]" requests

WORKDIR /app
COPY provision.py /app/provision.py
# new_app.py — one-off runbook to create a new application (project) + grant the
# SA PROJECT_OWNER, run via `--entrypoint python ... /app/new_app.py`.
COPY new_app.py /app/new_app.py

ENTRYPOINT ["python", "/app/provision.py"]
