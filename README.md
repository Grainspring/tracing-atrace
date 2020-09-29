![Tracing — Structured, application-level diagnostics][splash]

[splash]: https://raw.githubusercontent.com/tokio-rs/tracing/master/assets/splash.svg

# tracing-atrace

Support for logging [`tracing`][tracing] events natively to linux kernel debug tracing.

[![Crates.io][crates-badge]][crates-url]
[![Documentation (master)][docs-master-badge]][docs-master-url]
[![MIT licensed][mit-badge]][mit-url]
![maintenance status][maint-badge]

[crates-url]: https://crates.io/crates/tracing-atrace

## Overview

[`tracing`] is a framework for instrumenting Rust programs to collect
scoped, structured, and async-aware diagnostics. `tracing-atrace` provides a
[`tracing-subscriber::Layer`][layer] implementation for logging `tracing` spans
and events to [`linux kernel debug tracing`][kernel debug], on Linux
distributions that use debugfs.
 
*Compiler support: [requires `rustc` 1.40+][msrv]*

[msrv]: #supported-rust-versions
[`tracing`]: https://crates.io/crates/tracing

## Supported Rust Versions

Tracing is built against the latest stable release. The minimum supported
version is 1.40. The current Tracing version is not guaranteed to build on Rust
versions earlier than the minimum supported version.

Tracing follows the same compiler support policies as the rest of the Tokio
project. The current stable Rust compiler and the three most recent minor
versions before it will always be supported. For example, if the current stable
compiler version is 1.45, the minimum supported version will not be increased
past 1.42, three minor versions prior. Increasing the minimum supported compiler
version is not considered a semver breaking change as long as doing so complies
with this policy.

## License

This project is licensed under the [MIT license](LICENSE).

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Tracing by you, shall be licensed as MIT, without any additional
terms or conditions.
