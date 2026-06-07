# syntax=docker/dockerfile:1
FROM python:3-slim

# pyjwt[crypto] = pyjwt + cryptography (required RS256 backend, see spec §4.3).
RUN pip install --no-cache-dir "pyjwt[crypto]" requests

WORKDIR /app
COPY provision.py /app/provision.py

ENTRYPOINT ["python", "/app/provision.py"]
