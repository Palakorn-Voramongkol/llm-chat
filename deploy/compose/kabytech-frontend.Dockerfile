# syntax=docker/dockerfile:1
# kabytech-frontend: Next.js (App Router) standalone build. Build context =
# ./services/kabytech/frontend. Pin pnpm 9.15.9 (matches the package.json
# `packageManager` field and the lockfile format); pnpm 10/11 hard-fails on
# ignored native build scripts (sharp/unrs-resolver), pnpm 9 does not.
FROM node:20-alpine AS build
WORKDIR /app
RUN corepack enable && corepack prepare pnpm@9.15.9 --activate
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY . .
# Next bakes rewrites() destinations into the standalone build at BUILD time, so
# the in-network proxy target must be set here (runtime env is ignored for it).
ARG KABY_BACKEND_ORIGIN=http://kabytech-backend:7670
ENV KABY_BACKEND_ORIGIN=${KABY_BACKEND_ORIGIN}
RUN pnpm run build

FROM node:20-alpine
WORKDIR /app
ENV NODE_ENV=production
COPY --from=build /app/.next/standalone ./
COPY --from=build /app/.next/static ./.next/static
COPY --from=build /app/public ./public
EXPOSE 3000
CMD ["node", "server.js"]
