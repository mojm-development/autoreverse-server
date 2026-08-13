FROM node:22-slim AS build
WORKDIR /app
RUN corepack enable
COPY package.json pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile --ignore-scripts
COPY . .
RUN pnpm build
RUN pnpm prune --prod --ignore-scripts

FROM node:22-slim
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg \
  && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /app/build build
COPY --from=build /app/node_modules node_modules
COPY --from=build /app/package.json package.json
EXPOSE 3000
CMD ["node", "build"]
