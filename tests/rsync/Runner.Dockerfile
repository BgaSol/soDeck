FROM rust:1-slim-trixie@sha256:2752b332db73fdbb7dc576f06c82ed1f312005784ef913d7e04a28f5f55dc581
RUN sed -i 's|http://deb.debian.org|https://deb.debian.org|g' /etc/apt/sources.list.d/debian.sources && apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libssl-dev pkg-config build-essential perl && rm -rf /var/lib/apt/lists/*
WORKDIR /workspace
ENV CARGO_TARGET_DIR=/cargo-target
CMD ["cargo", "test", "--manifest-path", "src-tauri/Cargo.toml", "--lib", "rsync_", "--", "--ignored", "--test-threads=1"]
