![Tracing — Structured, application-level diagnostics][splash]

[splash]: https://raw.githubusercontent.com/tokio-rs/tracing/master/assets/splash.svg

# tracing-atrace

Support for logging [`tracing`][tracing] events natively to linux kernel debug tracing.

[crates-url]: https://crates.io/crates/tracing-atrace

## Overview

[`tracing`] is a framework for instrumenting Rust programs to collect
scoped, structured, and async-aware diagnostics. `tracing-atrace` provides a
[`tracing-atrace::Layer`][layer] implementation for logging `tracing` spans
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

## How To try tracing-atrace.<only for linux>
1.first compile tracing-atrace.

$cargo build

2.after compile correctly, check your linux kernel should support debugfs feature,and
then use the following commands to setup debufs  for tracing.

$sudo umount debugfs

$sudo mount -t debugfs none /sys/kernel/debug/

$sudo mount -t debugfs -o rw,mode=777,remount /sys/kernel/debug/

$sudo chmod -R 777 /sys/kernel/debug

3.go to target out dir, run atrace, if its output like the following, that's
good time for tracing.

$./atrace

4.capture your app's tracing. look example and atrace help for more
informations.

in one shell

$./atrace -T 10 > trace.log

in another shell

$./example

when atrace run finish, it'll get one trace.log.

5.open chrome browser,and enter chrome://tracing/ in url address.
in its load button, select trace.log, you'll get your tracing result.

tracing result example.
![chat tracing](http://grainspring.github.io/imgs/chat.tracing.png)

6.try tracing future

$cargo build --example chat

$cd target/debug/examples && ./chat

//for compress atrace log
$./atrace -T 30 -Z > atrace.log.z

$telnet localhost 6142

//uncompress atrace log
$./atrace -d atrace.log.z > atrace.log

//capture cpu schedule infos.
//it'll have big size log file with cpu scheduleinfos.
$./atrace -T 30 -Z --CPU_SCHED > atrace.log.z
![chat tracing with cpu schedule infos](http://grainspring.github.io/imgs/chat.tracing.with.cpu.sched.png)

7.tracing rustc examples
![rustc typeck_fn tracing](http://grainspring.github.io/imgs/tracing.rustc.typeck_fn.png)
![rustc borrowck tracing](http://grainspring.github.io/imgs/tracing.rustc.mir_borrowck.png)

Good time for tracing&profile futures.

## License

This project is licensed under the [MIT license](LICENSE).

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in Tracing by you, shall be licensed as MIT, without any additional
terms or conditions.
