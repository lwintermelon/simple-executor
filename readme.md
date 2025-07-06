# rust

rustup target list

rustc +nightly -Z unstable-options --print target-features --target=riscv64gc-unknown-none-elf

rustc +nightly -Z unstable-options --print target-spec-json

# zig

zig targets > zig_targets.json

zig build-exe -target riscv64-linux -ODebug -fsingle-threaded zig_hello.zig

zig build-exe -target riscv64-freestanding -ODebug hello.s --name hello.rv
