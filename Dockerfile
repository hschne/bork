# Conventional glibc release build. No host or persistent system configuration changes.
FROM rust:1.93.1-slim-bookworm@sha256:5b9332190bb3b9ece73b810cd1f1e9f06343b294ce184bcb067f0747d7d333ea AS build
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /build
ARG BUILD_JOBS=2
ENV CARGO_BUILD_JOBS=${BUILD_JOBS}
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY assets ./assets
RUN cargo build --release --locked
RUN install -Dm755 target/release/bork /artifact/bork

# `docker build --target artifact --output type=local,dest=dist .`
FROM scratch AS artifact
COPY --from=build /artifact/ /
COPY install.sh /

# CI checks and isolated preview verification of the release executable.
FROM build AS verify
RUN apt-get update && apt-get install -y --no-install-recommends \
    libwayland-client0 libwayland-cursor0 libxkbcommon0 libxkbcommon-x11-0 libegl1 libgl1 libgl1-mesa-dri \
    libx11-6 libxcursor1 libxi6 libxrandr2 fonts-dejavu-core xvfb xauth xdotool \
    && rm -rf /var/lib/apt/lists/*
RUN rustup component add clippy rustfmt
COPY tests ./tests
ENV LIBGL_ALWAYS_SOFTWARE=1
CMD ["/artifact/bork", "--version"]
