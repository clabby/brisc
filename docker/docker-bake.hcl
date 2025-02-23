variable "REGISTRY" {
  default = "ghcr.io"
}

variable "REPOSITORY" {
  default = "clabby/brisc"
}

variable "DEFAULT_TAG" {
  default = "brisc:local"
}

variable "PLATFORMS" {
  // Only specify a single platform when `--load` ing into docker.
  // Multi-platform is supported when outputting to disk or pushing to a registry.
  // Multi-platform builds can be tested locally with:  --set="*.output=type=image,push=false"
  default = "linux/amd64,linux/arm64"
}

// Special target: https://github.com/docker/metadata-action#bake-definition
target "docker-metadata-action" {
  tags = ["${DEFAULT_TAG}"]
}

target "riscv-unknown-elf-toolchain" {
  inherits = ["docker-metadata-action"]
  context = "."
  dockerfile = "docker/riscv-unknown-elf-toolchain.dockerfile"
  platforms = split(",", PLATFORMS)
}

target "rust-riscv-cross" {
  inherits = ["docker-metadata-action"]
  context = "."
  dockerfile = "docker/rust-riscv-cross.dockerfile"
  platforms = split(",", PLATFORMS)
}
