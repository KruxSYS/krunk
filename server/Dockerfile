# krunk-sync API image (docs/sync/PLAN.md §4.1): build with Rust, run on distroless as non-root.
FROM rust:1.99.1-slim-trixie@sha256:ea0999ff67355d958b8799b6751c879a6d770076d26047dca66f6e51e77930d3 AS build
WORKDIR /src
# Dependencies first, so source edits don't refetch the registry.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo fetch --locked \
    && rm -rf src
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release --locked --offline \
    && cp target/release/krunk-sync /krunk-sync \
    && mkdir -p /out/secrets

FROM gcr.io/distroless/cc-debian13:nonroot@sha256:08476c244888bfb425bae1ae4dd7c17f093e972aab8426883d97f3c6f280f7c1
COPY --from=build /krunk-sync /krunk-sync
# Mount point of the krunk-secrets volume; a new volume inherits this owner (init-secrets).
COPY --from=build --chown=65532:65532 /out/secrets /secrets
USER 65532:65532
EXPOSE 8081
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 CMD ["/krunk-sync", "healthcheck"]
ENTRYPOINT ["/krunk-sync"]
CMD ["serve"]
