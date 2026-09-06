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
RUN npm run build --workspace=web

# Runtime enxuto
FROM node:20-slim@sha256:2cf067cfed83d5ea958367df9f966191a942351a2df77d6f0193e162b5febfc0
WORKDIR /app
ENV NODE_ENV=production
COPY --from=builder /app/package.json /app/package-lock.json* ./
COPY --from=builder /app/apps/web/package.json apps/web/
COPY --from=builder /app/node_modules ./node_modules
COPY --from=builder /app/apps/web/.next ./apps/web/.next
COPY --from=builder /app/apps/web/next.config.ts ./apps/web/
COPY --from=builder /app/apps/web/app ./apps/web/app
EXPOSE 3000
WORKDIR /app/apps/web
CMD ["npm", "run", "start"]
