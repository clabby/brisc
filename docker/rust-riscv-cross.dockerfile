FROM runtimeverificationinc/riscv-gnu-toolchain:ubuntu-jammy-2024.04.12 AS rv-bare

FROM ubuntu:22.04

ARG ARCH
ARG TARGET_FEATURES

ENV SHELL=/bin/bash
ENV DEBIAN_FRONTEND=noninteractive

# Install core dependencies
RUN apt-get update && apt-get install --assume-yes --no-install-recommends \
  ca-certificates \
  build-essential \
  autoconf \
  automake \
  autotools-dev \
  git \
  curl \
  make \
  cmake \
  xxd \
  python3 \
  g++-riscv64-linux-gnu \
  libc6-dev-riscv64-cross \
  binutils-riscv64-linux-gnu \
  llvm \
  clang

# Install multilib support for the RISC-V GNU toolchain
RUN apt-get install gcc-multilib --assume-yes --no-install-recommends

# Copy the RISC-V GNU toolchain from the previous stage
COPY --from=rv-bare /opt/riscv /opt/riscv

# Install Rustup and Rust
ENV RUST_VERSION=nightly
ENV PATH="/root/.cargo/bin:${PATH}"
RUN curl https://sh.rustup.rs -sSf | bash -s -- -y --default-toolchain ${RUST_VERSION} --component rust-src

# Set up the env vars to instruct rustc to use the correct compiler and linker
# and to build correctly to support the Cannon processor
ENV CC_riscv32_unknown_none_elf=riscv64-linux-gnu-gcc \
  CC_riscv64_unknown_none_elf=riscv64-linux-gnu-gcc \
  CXX_riscv32_unknown_none_elf=riscv64-linux-gnu-g++ \
  CXX_riscv64_unknown_none_elf=riscv64-linux-gnu-g++ \
  CARGO_TARGET_RISCV32_UNKNOWN_NONE_ELF_LINKER=riscv64-linux-gnu-gcc \
  CARGO_TARGET_RISCV64_UNKNOWN_NONE_ELF_LINKER=riscv64-linux-gnu-gcc \
  RUSTFLAGS="-Clink-arg=-e_start -Ctarget-feature=$TARGET_FEATURES" \
  CARGO_BUILD_TARGET="$ARCH-unknown-none-elf" \
  RUSTUP_TOOLCHAIN=${RUST_VERSION}
