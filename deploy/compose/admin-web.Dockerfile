# syntax=docker/dockerfile:1
# admin-web: Next.js (App Router) standalone build. Build context = ./admin-web.
# Pin pnpm 9 (the version that produced pnpm-lock.yaml's v9.0 format). Without a
# pin, `corepack enable` resolves the latest pnpm 10, which (a) imports the
# node:sqlite built-in that only exists on Node 22+, so the install fails on
# Node 20 with ERR_UNKNOWN_BUILTIN_MODULE, and (b) refuses dependency build
# scripts (sharp/msw/unrs-resolver) with ERR_PNPM_IGNORED_BUILDS. pnpm 9 has
# neither issue and matches the lockfile.
FROM node:20-alpine AS build
WORKDIR /app
RUN corepack enable && corepack prepare pnpm@9.15.9 --activate
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY . .
# Next.js bakes rewrites() destinations into the build manifest at BUILD time
# (output: standalone). The runtime ADMIN_API_ORIGIN env is therefore ignored
# for the /api,/login,/callback,/logout proxy, so the in-network target must be
# set here. Defaults to the compose service name; override with --build-arg.
ARG ADMIN_API_ORIGIN=http://admin-api:7676
ENV ADMIN_API_ORIGIN=${ADMIN_API_ORIGIN}
RUN pnpm run build

FROM node:20-alpine
WORKDIR /app
ENV NODE_ENV=production
# next.config sets output:'standalone' -> a self-contained server bundle.
COPY --from=build /app/.next/standalone ./
COPY --from=build /app/.next/static ./.next/static
COPY --from=build /app/public ./public
EXPOSE 3000
CMD ["node", "server.js"]
