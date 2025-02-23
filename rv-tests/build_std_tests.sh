export RISCV=/opt/riscv
cd riscv-tests

autoupdate
autoconf
./configure --prefix=$RISCV/target
RISCV_PREFIX="$RISCV/bin/riscv64-unknown-elf-" make isa
