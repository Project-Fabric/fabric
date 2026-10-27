# Cosigna gateway, self-hosted. Build from the repository root:
#   docker build -f deploy/docker/Dockerfile -t cosigna-gateway .
FROM rust:1.99.1-slim-bookworm AS build
# Use the image's toolchain; rust-toolchain.toml would also download rustfmt and clippy.
ENV RUSTUP_TOOLCHAIN=1.99.1
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates crates
RUN cargo build --release --locked -p cosigna-agent \
 && install -m 0755 target/release/cosigna-agent /cosigna-agent

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin cosigna \
 && mkdir -p /data \
 && chown cosigna:cosigna /data \
 && chmod 0700 /data
COPY --from=build /cosigna-agent /usr/local/bin/cosigna-agent
COPY --chmod=0755 deploy/docker/entrypoint.sh /usr/local/bin/cosigna-entrypoint
USER cosigna
VOLUME /data
EXPOSE 3092
HEALTHCHECK --interval=15s --timeout=5s --start-period=10s CMD ["cosigna-agent", "probe", "--port", "3092"]
ENTRYPOINT ["cosigna-entrypoint"]
CMD ["serve", "--listen", "0.0.0.0:3092", "--public-host", "localhost"]
