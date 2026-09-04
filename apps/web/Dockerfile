# Placeholder: Next.js real entra em slice posterior (docs/frontend.md).
FROM node:20-slim
WORKDIR /app
COPY apps/web/package.json ./
RUN npm install --omit=dev || true
EXPOSE 3000
CMD ["npm", "run", "dev"]
