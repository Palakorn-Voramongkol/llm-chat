# syntax=docker/dockerfile:1
# admin-web: Next.js (App Router) standalone build. Build context = ./admin-web.
FROM node:20-alpine AS build
WORKDIR /app
RUN corepack enable
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY . .
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
