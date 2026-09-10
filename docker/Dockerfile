# agentic-eda: self-contained engine image.
#   eda (Rust) + Cypress/DREAMPlace CPU placer (patched) + example intents.
# No KiCad inside: KiCad is a viewer on the host (bench/loop/render3d.py).
#
#   docker build -f docker/Dockerfile -t agentic-eda .
#   docker run --rm -v "$PWD":/work agentic-eda pipeline my_board.yaml -o out --placer cypress

# ---- stage 1: Rust engine -------------------------------------------------
FROM rust:1-bookworm AS engine
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p eda-cli && strip target/release/eda

# ---- stage 2: Cypress placer (CPU) ----------------------------------------
FROM python:3.9-bookworm AS cypress
ARG CYPRESS_REV=743b07c4e33dc52a00584d55d2e67f38f70957f5
RUN apt-get update && apt-get install -y --no-install-recommends \
        cmake ninja-build flex libfl-dev bison libboost-all-dev libcairo2-dev zlib1g-dev git \
    && rm -rf /var/lib/apt/lists/*
RUN pip install --no-cache-dir --only-binary=:all: --extra-index-url https://download.pytorch.org/whl/cpu torch==2.2.2 torchvision==0.17.2 \
    && pip install --no-cache-dir --only-binary=:all: numpy==1.26.4 scipy==1.13.1 matplotlib==3.9.4 cairocffi==1.7.1 shapely==2.0.7 pillow
RUN git clone --recursive https://github.com/NVlabs/Cypress /Cypress \
    && cd /Cypress && git checkout -q $CYPRESS_REV && git submodule update --init --recursive
COPY docker/cypress/cypress-cpu.patch docker/cypress/limbo.patch /tmp/
RUN cd /Cypress && git apply /tmp/cypress-cpu.patch \
    && cd thirdparty/Limbo && git apply /tmp/limbo.patch
RUN cd /Cypress && mkdir build && cd build \
    && cmake .. -G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/opt/cypress \
         -DPYTHON_EXECUTABLE=$(which python) -DCMAKE_CXX_ABI=0 \
    && ninja -j2 install \
    && rm -rf /Cypress/build

# ---- stage 3: runtime -----------------------------------------------------
FROM python:3.9-slim-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends libcairo2 libgomp1 libboost-iostreams1.74.0 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=cypress /usr/local/lib/python3.9/site-packages /usr/local/lib/python3.9/site-packages
COPY --from=cypress /opt/cypress /opt/cypress
COPY --from=engine /src/target/release/eda /usr/local/bin/eda
COPY examples /opt/agentic-eda/examples
COPY bench/loop /opt/agentic-eda/bench/loop
ENV CYPRESS_INSTALL=/opt/cypress CYPRESS_PYTHON=python3 EDA_EXAMPLES=/opt/agentic-eda/examples
WORKDIR /work
ENTRYPOINT ["eda"]
