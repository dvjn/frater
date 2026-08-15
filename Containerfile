# distroless has no shell, so the data directory is prepared in a stage that
# has one. The stage is pinned to the build platform, because it only makes a
# directory and its own architecture never reaches the runtime image.
FROM --platform=$BUILDPLATFORM docker.io/library/busybox:1.37.0 AS layout
RUN install -d -o 65532 -g 65532 -m 0700 /out/data

FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
ARG TARGETARCH

COPY dist/${TARGETARCH}/frater /usr/local/bin/frater
COPY --from=layout --chown=65532:65532 /out/data /data

USER 65532:65532
WORKDIR /data
VOLUME ["/data"]
EXPOSE 3000

ENV HTTP_ADDR=0.0.0.0:3000 \
    DATABASE_URL="sqlite:///data/frater.db?mode=rwc" \
    SECRET_KEY=@/data/root.key \
    RUST_LOG=frater=info,tower_http=info

ENTRYPOINT ["/usr/local/bin/frater"]
