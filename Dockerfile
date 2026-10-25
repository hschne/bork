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
RUN install -Dm755 target/release/bork /artifact/bork \
    && install -Dm644 assets/overtime.png /artifact/assets/overtime.png

# `docker build --target artifact --output type=local,dest=dist .`
FROM scratch AS artifact
COPY --from=build /artifact/ /
COPY LICENSE THIRD_PARTY_LICENSES.txt README.md /
COPY assets/intermission.png /assets/intermission.png

# Disposable second-distribution GUI/runtime verification, not the shipped artifact.
FROM build AS verify
RUN apt-get update && apt-get install -y --no-install-recommends \
    libwayland-client0 libwayland-cursor0 libxkbcommon0 libxkbcommon-x11-0 libegl1 libgl1 libgl1-mesa-dri \
    libx11-6 libxcursor1 libxi6 libxrandr2 fonts-dejavu-core xvfb xauth xdotool grim \
    && rm -rf /var/lib/apt/lists/*
RUN rustup component add clippy rustfmt
COPY tests ./tests
RUN cargo test --locked --test display --no-run --message-format=json > /tmp/display-build.json \
    && executable="$(sed -n '/"name":"display"/s/.*"executable":"\([^"]*\)".*/\1/p' /tmp/display-build.json)" \
    && test -n "$executable" && cp "$executable" /artifact/display-test \
    && rm /tmp/display-build.json
ENV LIBGL_ALWAYS_SOFTWARE=1
CMD ["/artifact/bork", "--version"]
