# ndk-glue

[![Actions Status](https://github.com/rust-mobile/ndk-glue/actions/workflows/rust.yml/badge.svg)](https://github.com/rust-mobile/ndk-glue/actions/workflows/rust.yml)
[![Latest version](https://img.shields.io/crates/v/ndk-glue.svg?logo=rust)](https://crates.io/crates/ndk-glue)
[![MSRV](https://img.shields.io/badge/rustc-1.80.0+-ab6000.svg)](https://blog.rust-lang.org/2024/07/25/Rust-1.80.0.html)
[![Documentation](https://docs.rs/ndk-glue/badge.svg)](https://docs.rs/ndk-glue)
[![Lines of code](https://tokei.rs/b1/github/rust-mobile/ndk-glue)](https://github.com/rust-mobile/ndk-glue)
![MIT](https://img.shields.io/badge/License-MIT-green.svg)
![Apache 2.0](https://img.shields.io/badge/License-Apache_2.0-green.svg)

Integration layer and helper macros for delegating Android `NativeActivity` launches (`ANativeActivity_onCreate()`) in a safe Rust-like interface.  Useful to build windowing abstractions on top.

This library exports the `main` attribute macro from `ndk-macro`, see the corresponding README for more details.
