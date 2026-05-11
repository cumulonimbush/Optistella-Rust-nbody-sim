# Usage

If using native rust, use `cargo run` to run and `cargo build` to build etc. WSL setup can be found below.

## WSL Requirements For Cross Compilation to Windows

`cargo install cargo-alias-exec cargo-xwin`

`rustup target add x86_64-pc-windows-msvc`

You must install `lld` and `llvm`. A C compiler is a must too but it is yor choosing, though, `clang` is highly advised.

## WSL Commands

`cargo xwinb`: Dev build for windows.

`cargo xrun`: Dev build & run for windows.

`cargo xrel`: Relase build for windows.
