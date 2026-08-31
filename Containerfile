# scratch has no shell, so the binary and data directory are installed in a
# stage that has one. The stage is pinned to the build platform, because its
# own architecture never reaches the runtime image.
FROM --platform=$BUILDPLATFORM docker.io/library/busybox:1.38.0@sha256:dc2d74b28e4cf8984fa52af1f39bc7c3d9c73760b41a74d629f5d11b1ab28616 AS layout
ARG TARGETARCH
COPY dist/${TARGETARCH}/frater /tmp/frater
RUN install -Dm0555 /tmp/frater /out/usr/local/bin/frater \
    && install -d -o 65532 -g 65532 -m 0700 /out/data

FROM scratch

COPY --from=layout /out/ /

USER 65532:65532
WORKDIR /data
VOLUME ["/data"]
EXPOSE 3210

ENV HTTP_ADDR=0.0.0.0:3210 \
    DATABASE_URL="sqlite:///data/frater.db?mode=rwc" \
    SECRET_KEY=@/data/root.key \
    RUST_LOG=frater=info,tower_http=info

ENTRYPOINT ["/usr/local/bin/frater"]
