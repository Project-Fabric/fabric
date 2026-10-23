FROM rust:1.99.1-bookworm@sha256:48b7f435b8fc6378ce43b44f15e3a47fd8919b2825f835c39bd4bd3da50af5b8 AS rust-build
WORKDIR /opt/cosigna
ENV CARGO_BUILD_JOBS=4
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
RUN cargo test -p cosigna-agent --locked --no-run \
    && mkdir -p /bundle/tests \
    && for suite in crash_recovery gateway transactions pipeline worker http planner; do \
         artifact="$(find target/debug/deps -maxdepth 1 -type f -name "${suite}-*" -executable)"; \
         test -n "$artifact"; \
         cp "$artifact" "/bundle/tests/$suite"; \
         strip "/bundle/tests/$suite"; \
       done \
    && strip target/debug/cosigna-agent

FROM node:24.21.0-bookworm-slim@sha256:d6aa754f16b3197301076f047b5def2f02ea1dbbc2ca920407d46d7ec7f87b20 AS node-runtime

FROM apache/airflow:3.3.2-python3.12@sha256:9df9c8be4096b9cc626bd7cb1f2b8c712eef66c59c615f8c7e6200871ca10bd1
USER root
COPY --from=node-runtime /usr/local/bin/node /usr/local/bin/node
COPY --from=rust-build /bundle/tests /opt/cosigna/tests
# Rust integration tests embed this original executable path at compile time.
COPY --from=rust-build /opt/cosigna/target/debug/cosigna-agent /opt/cosigna/target/debug/cosigna-agent
COPY scripts/check-gateway-service.mjs /opt/cosigna/scripts/check-gateway-service.mjs
COPY testbeds/pipeline-lab/verification_runner.py /opt/cosigna/verification_runner.py
COPY testbeds/pipeline-lab/dags /opt/cosigna/dags
RUN mkdir -p /opt/cosigna/reports && chown airflow:root /opt/cosigna/reports
USER airflow
ENV PYTHONPATH=/opt/cosigna \
    AIRFLOW_HOME=/tmp/cosigna-airflow \
    AIRFLOW__CORE__DAGS_FOLDER=/opt/cosigna/dags \
    AIRFLOW__CORE__LOAD_EXAMPLES=False
