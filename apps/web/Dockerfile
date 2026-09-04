# Placeholder: Next.js real entra no slice da UI (docs/frontend.md).
FROM node:20-slim
WORKDIR /app
COPY package.json ./
RUN npm install --omit=dev || true
EXPOSE 3000
CMD ["npm", "run", "dev"]
