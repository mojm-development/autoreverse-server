# syntax=docker/dockerfile:1

# --- Build ------------------------------------------------------------------
# Runs on the build machine's own architecture and cross-compiles for the target: an arm64 image
# built on an amd64 runner (or the reverse) would otherwise compile Rust under QEMU, which takes
# about ten times as long.
FROM --platform=$BUILDPLATFORM rust:1.99-slim-trixie AS build
ARG BUILDARCH
ARG TARGETARCH
RUN set -eu; \
	case "$TARGETARCH" in \
		amd64) triple=x86_64-unknown-linux-gnu; gcc=gcc-x86-64-linux-gnu; libc=libc6-dev-amd64-cross ;; \
		arm64) triple=aarch64-unknown-linux-gnu; gcc=gcc-aarch64-linux-gnu; libc=libc6-dev-arm64-cross ;; \
		*) echo "Nicht unterstützte Architektur: $TARGETARCH" >&2; exit 1 ;; \
	esac; \
	echo "$triple" > /triple; \
	if [ "$TARGETARCH" != "$BUILDARCH" ]; then \
		apt-get update; \
		apt-get install -y --no-install-recommends "$gcc" "$libc"; \
		rm -rf /var/lib/apt/lists/*; \
		rustup target add "$triple"; \
	fi
# ring (rustls) compiles C. Debian's native gcc also answers to the triplet name, so these hold
# for native and cross builds alike.
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
	CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
	CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
	CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
WORKDIR /src

# Dependencies first, against stub sources: this layer only rebuilds when Cargo.toml or
# Cargo.lock change, not on every code change. A plain layer (not a cache mount) so the CI's
# GitHub Actions cache carries it.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && touch src/lib.rs \
	&& cargo build --release --locked --target "$(cat /triple)" \
	&& rm -rf src

COPY src src
COPY templates templates
COPY migrations migrations
# Templates (askama) and migrations (sqlx::migrate!) are compiled into the binary.
RUN touch src/main.rs src/lib.rs \
	&& cargo build --release --locked --target "$(cat /triple)" \
	&& cp "target/$(cat /triple)/release/autoreverse-server" /autoreverse-server

# --- Runtime ----------------------------------------------------------------
FROM debian:trixie-slim
# ffprobe/ffmpeg: m4b chapters and podcast chapter extraction. ca-certificates: podcast feeds
# and downloads over HTTPS.
RUN apt-get update \
	&& apt-get install -y --no-install-recommends ffmpeg ca-certificates \
	&& rm -rf /var/lib/apt/lists/*
# UID 1000 is the first user on most Linux hosts, so bind-mounted folders (the podcast
# downloads) are usually writable without extra steps. Otherwise: `user:` in compose.
RUN groupadd --gid 1000 autoreverse \
	&& useradd --uid 1000 --gid autoreverse --home-dir /data --no-create-home --shell /usr/sbin/nologin autoreverse \
	&& mkdir -p /data \
	&& chown autoreverse:autoreverse /data
WORKDIR /app
COPY --from=build /autoreverse-server /app/autoreverse-server
COPY static /app/static
# Migrations always run at start (they are embedded) — the old AUTOREVERSE_AUTO_MIGRATE switch
# is gone.
ENV AUTOREVERSE_STATIC=/app/static \
	AUTOREVERSE_DATA=/data \
	HOST=0.0.0.0 \
	PORT=3000
USER autoreverse
EXPOSE 3000
HEALTHCHECK --interval=10s --timeout=5s --start-period=10s --retries=3 \
	CMD ["/app/autoreverse-server", "healthcheck"]
CMD ["/app/autoreverse-server"]
