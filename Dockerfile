# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
# The MuxMultiTool container image: the static musl server binary next to a
# static GPL ffmpeg, on `scratch`. No shell, no package manager, no libc.
#
# The Dockerfile assembles, it does not compile. The server is cross-compiled
# on the host for both targets (cargo-zigbuild, wrapped in cargo-auditable so
# the dependency list is embedded for image scanners) and staged per
# architecture, and the frontend is built with vite:
#
#   cargo auditable zigbuild --release --locked -p muxmultitool-server \
#     --target x86_64-unknown-linux-musl --target aarch64-unknown-linux-musl
#   mkdir -p dist/amd64 dist/arm64
#   cp target/x86_64-unknown-linux-musl/release/muxmultitool-server dist/amd64/
#   cp target/aarch64-unknown-linux-musl/release/muxmultitool-server dist/arm64/
#   (cd web && npm ci && npm run build)
#   docker buildx build --platform linux/amd64,linux/arm64 -t muxmultitool .
#
# `.github/workflows/image.yml` runs exactly this. `TARGETARCH` picks the
# binary, so one build produces both platforms without emulation.
#
# ffmpeg: mwader/static-ffmpeg, a fully static (musl) GPL build with x264,
# configured `--enable-gpl --enable-version3`, published for amd64 and arm64
# and pinned by digest (Renovate updates the tag and digest together). The
# BtbN linux builds named in docs/architecture.md link glibc dynamically, so
# they would need a glibc base instead of `scratch`.
ARG FFMPEG_IMAGE=docker.io/mwader/static-ffmpeg:9.0.2-1@sha256:f5d100184884eb3e82b0eb14bb5ebf98da99826ac824989720297fb9ef21c822
ARG LAYOUT_IMAGE=docker.io/library/busybox:1.38.0-musl@sha256:ea2b9914a16a4ac1981994af97b318f7c7d4db76b580c56177f08bf76f4a0be8

# The tag and digest are in the ARG above, which hadolint does not read (DL3006).
# hadolint ignore=DL3006
FROM ${FFMPEG_IMAGE} AS ffmpeg

# `scratch` has no mkdir: the folders the server needs (a writable /tmp for
# preview segments and sprites, /data for the settings, /media as the mount
# point) are laid out in a throwaway stage and copied over with their modes.
# This stage runs on the build platform, whatever the target is.
# hadolint ignore=DL3006
FROM --platform=${BUILDPLATFORM} ${LAYOUT_IMAGE} AS layout
ARG TARGETARCH
RUN mkdir -p /rootfs/tmp /rootfs/data /rootfs/media /rootfs/usr/local/bin \
	&& chmod 1777 /rootfs/tmp
COPY --chmod=755 dist/${TARGETARCH}/muxmultitool-server /rootfs/usr/local/bin/muxmultitool-server
COPY web/dist /rootfs/usr/local/share/muxmultitool/web

FROM scratch

ARG FFMPEG_IMAGE
LABEL org.opencontainers.image.title="MuxMultiTool" \
	org.opencontainers.image.description="Self-hostable ffmpeg GUI: lossless cutting, remuxing and stream selection" \
	org.opencontainers.image.source="https://github.com/Jogai/MuxMultiTool" \
	org.opencontainers.image.licenses="GPL-3.0-or-later" \
	org.opencontainers.image.base.name="${FFMPEG_IMAGE}"

# ffmpeg and ffprobe, with the CA bundle (https inputs) and the fontconfig
# setup, fonts and font cache (subtitle and drawtext filters; without the
# cache fontconfig would scan the fonts on every start and try to write a
# cache into a home folder the user does not have). The HTML docs (/doc)
# and the console fonts stay out.
COPY --from=ffmpeg /ffmpeg /ffprobe /usr/local/bin/
COPY --from=ffmpeg /etc/ssl/cert.pem /etc/ssl/cert.pem
COPY --from=ffmpeg /etc/fonts /etc/fonts
COPY --from=ffmpeg /usr/share/fonts /usr/share/fonts
COPY --from=ffmpeg /var/cache/fontconfig /var/cache/fontconfig
# The versions of the libraries linked into ffmpeg, which no scanner can see
# in a static binary.
COPY --from=ffmpeg /versions.json /usr/local/share/muxmultitool/ffmpeg-versions.json
COPY --from=layout /rootfs/usr /usr
COPY --from=layout /rootfs/tmp /tmp
COPY --from=layout /rootfs/media /media
COPY --from=layout --chown=65532:65532 /rootfs/data /data

# Every setting is an environment variable (see `muxmultitool-server --help`).
# These are the container defaults; override them in `docker run -e` or compose.
ENV MMT_LISTEN=0.0.0.0 \
	MMT_PORT=8080 \
	MMT_MEDIA_ROOTS=/media \
	MMT_DATA_DIR=/data \
	MMT_JOBS=1 \
	MMT_FRONTEND_DIR=/usr/local/share/muxmultitool/web \
	MMT_FFMPEG=/usr/local/bin/ffmpeg \
	MMT_FFPROBE=/usr/local/bin/ffprobe \
	TMPDIR=/tmp \
	RUST_LOG=info

# An unprivileged user (the distroless "nonroot" ids). Mounted media folders
# must be writable by it, or run with `--user` matching the folders' owner.
USER 65532:65532
EXPOSE 8080

# The health check calls the health RPC through the server binary itself:
# there is no shell or curl in the image, and no separate health endpoint.
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
	CMD ["/usr/local/bin/muxmultitool-server", "health"]

# PID 1 is the server. It handles SIGTERM (what `docker stop` sends): every
# running ffmpeg job is cancelled, killed and waited for before it exits.
ENTRYPOINT ["/usr/local/bin/muxmultitool-server"]
