FROM --platform=${BUILDPLATFORM} rust:latest AS chef

RUN update-ca-certificates

RUN cargo install cargo-chef

# planner
FROM chef AS planner

COPY . .

#RUN rm -f rust-toolchain.toml

RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS cook

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl-dev \
    pkgconf \
    gcc-aarch64-linux-gnu \
    gcc-x86-64-linux-gnu \
    libc6-dev-arm64-cross \
    libc6-dev-amd64-cross \
    && rm -rf /var/lib/apt/lists/


ARG TARGETPLATFORM

ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc

#COPY rust-toolchain.toml rust-toolchain.toml

COPY --from=planner recipe.json recipe.json

COPY target.sh target.sh

RUN . ./target.sh && rustup target add $RUST_TARGET
RUN . ./target.sh && cargo chef cook --release --target $RUST_TARGET --recipe-path recipe.json


FROM cook AS buildah

# Create appuser
ENV USER=app
ENV UID=10001

ARG BIN_NAME

RUN adduser \
    --disabled-password \
    --gecos "" \
    --home "/nonexistent" \
    --shell "/sbin/nologin" \
    --no-create-home \
    --uid "${UID}" \
    "${USER}"

WORKDIR /buildah

# copy the whole repo into the buildah container
COPY . .

COPY target.sh target.sh

RUN . ./target.sh && echo "Building for $TARGET_ARCH" && cargo build --release --target $RUST_TARGET && cp -r target/$RUST_TARGET/release/ target/

FROM --platform=${TARGETARCH:-$BUILDPLATFORM} gcr.io/distroless/cc

ARG BIN_NAME

# Import from builder.
COPY --from=buildah /etc/passwd /etc/passwd
COPY --from=buildah /etc/group /etc/group

WORKDIR /app

# set which executable to copy over based on BIN_NAME
COPY --from=buildah /buildah/$BIN_NAME ./$BIN_NAME

# Use an unprivileged user.
USER app:app

CMD ["ls"]
