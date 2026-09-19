# Build real Next.js (Fatia 1.5). Contexto de build = raiz do repo (infra/compose.yaml).
FROM node:20-slim@sha256:2cf067cfed83d5ea958367df9f966191a942351a2df77d6f0193e162b5febfc0 AS builder
WORKDIR /app

# Manifests primeiro (cache de dependências; workspaces na raiz)
COPY package.json package-lock.json* ./
COPY apps/web/package.json apps/web/
RUN npm ci --include=dev 2>/dev/null || npm install --include=dev

# Resto do app web e build
COPY apps/web/ apps/web/
ARG NEXT_PUBLIC_API_URL=http://localhost:8080
ENV NEXT_PUBLIC_API_URL=$NEXT_PUBLIC_API_URL
ARG API_INTERNAL_URL=http://principal:8080
ENV API_INTERNAL_URL=$API_INTERNAL_URL
RUN npm run build --workspace=web

# Runtime enxuto (standalone + usuário não-root)
FROM node:20-slim@sha256:2cf067cfed83d5ea958367df9f966191a942351a2df77d6f0193e162b5febfc0
WORKDIR /app
ENV NODE_ENV=production
ENV PORT=3000
ENV HOSTNAME="0.0.0.0"
RUN groupadd -g 1001 nodejs && useradd -u 1001 -g nodejs nextjs
COPY --from=builder /app/apps/web/public ./apps/web/public
COPY --from=builder --chown=nextjs:nodejs /app/apps/web/.next/standalone ./
COPY --from=builder --chown=nextjs:nodejs /app/apps/web/.next/static ./apps/web/.next/static
USER nextjs
EXPOSE 3000
WORKDIR /app/apps/web
CMD ["node", "server.js"]
