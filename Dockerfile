FROM oven/bun:1 AS build
WORKDIR /app
COPY package.json bun.lock ./
RUN bun install --frozen-lockfile --ignore-scripts
COPY . .
RUN bun run build
# Prod-only node_modules for the runtime stage (adapter-node's output runs on Node).
RUN rm -rf node_modules && bun install --frozen-lockfile --production --ignore-scripts

FROM node:22-slim
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg \
  && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/build build
COPY --from=build /app/node_modules node_modules
COPY --from=build /app/package.json package.json
EXPOSE 3000
CMD ["node", "build"]
