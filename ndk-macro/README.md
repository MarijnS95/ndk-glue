# ndk-macro

[![Actions Status](https://github.com/rust-mobile/ndk-glue/actions/workflows/rust.yml/badge.svg)](https://github.com/rust-mobile/ndk-glue/actions/workflows/rust.yml)
[![Latest version](https://img.shields.io/crates/v/ndk-macro.svg?logo=rust)](https://crates.io/crates/ndk-macro)
[![MSRV](https://img.shields.io/badge/rustc-1.80.0+-ab6000.svg)](https://blog.rust-lang.org/2024/07/25/Rust-1.80.0.html)
[![Documentation](https://docs.rs/ndk-macro/badge.svg)](https://docs.rs/ndk-macro)
[![Lines of code](https://tokei.rs/b1/github/rust-mobile/ndk-glue)](https://github.com/rust-mobile/ndk-glue)
![MIT](https://img.shields.io/badge/License-MIT-green.svg)
![Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-green.svg)

Implementation of the attribute procedural macro `main` which applied directly to main function.

This macro is re-exported in `ndk-glue`. Typically, it's not needed to depend on this library directly!

## Usage

```rust
#[cfg_attr(target_os = "android", ndk_glue::main(backtrace = "on"))]
pub fn main() {
    println!("hello world");
}
```

The attribute macro supports optional input attributes:

- `backtrace = "on|full"`: Enables backtraces by setting the `RUST_BACKTRACE` env var
- `ndk_glue = "path::to::ndk_glue"`: Overrides default path to __ndk_glue__ crate
- `logger(...props)`: Configures android logger with the passed configuration (requires the `logger` feature):
  - `level = "error|warn|info|debug|trace"`: Changes log level for logger
  - `tag = "my-tag"`: Assigns tag to logger
  - `filter = "filtering-rules"`: Changes default filtering rules
