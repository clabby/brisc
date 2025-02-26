export RISCV=/opt/riscv

(cd riscv-tests && \
  autoupdate && \
  autoconf && \
  ./configure --prefix=$RISCV/target && \
  RISCV_PREFIX="$RISCV/bin/riscv64-unknown-elf-" make isa)

# RV32
cp riscv-tests/isa/rv32ui-p-* bin
cp riscv-tests/isa/rv32um-p-* bin
cp riscv-tests/isa/rv32ua-p-* bin
cp riscv-tests/isa/rv32uc-p-* bin

# RV64
cp riscv-tests/isa/rv64ui-p-* bin
cp riscv-tests/isa/rv64um-p-* bin
cp riscv-tests/isa/rv64ua-p-* bin
cp riscv-tests/isa/rv64uc-p-* bin
