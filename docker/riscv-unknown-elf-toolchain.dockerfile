FROM ubuntu:22.04

# Install core build dependencies
RUN apt-get update && \
  apt-get install --assume-yes --no-install-recommends \
  ca-certificates \
  autoconf \
  automake \
  autotools-dev \
  curl \
  python3 \
  python3-pip \
  python3-tomli \
  libmpc-dev \
  libmpfr-dev \
  libgmp-dev \
  gawk \
  build-essential \
  bison \
  flex \
  texinfo \
  gperf \
  libtool \
  patchutils \
  bc \
  zlib1g-dev \
  libexpat-dev \
  ninja-build \
  git \
  cmake \
  libglib2.0-dev \
  libslirp-dev

ENV RISCV=/opt/riscv
ENV PATH=$PATH:$RISCV/bin

ARG TOOLCHAIN_VERSION=2024.04.12
RUN git clone --recursive https://github.com/riscv/riscv-gnu-toolchain && \
    cd riscv-gnu-toolchain && \
    ./configure --prefix=$RISCV --enable-multilib && \
    make && \
    cd .. && \
    rm -rf riscv-gnu-toolchain
