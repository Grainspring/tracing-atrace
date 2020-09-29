use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write as IoWrite;
use std::io::{Error, ErrorKind};
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};

const UNINITIALIZED: usize = 0;
const INITIALIZING: usize = 1;
const INITIALIZED: usize = 2;

static TRACE_WRITER_INIT: AtomicUsize = AtomicUsize::new(UNINITIALIZED);

static mut GLOBAL_TRACE_WRITER: Option<TraceWriter> = None;

struct TraceWriter {
    file: File,
}

pub fn init_trace_writer() -> Result<(), Error> {
    #[cfg(unix)]
    if TRACE_WRITER_INIT.load(Ordering::SeqCst) == INITIALIZED {
        Ok(())
    } else if TRACE_WRITER_INIT.compare_and_swap(UNINITIALIZED, INITIALIZING, Ordering::SeqCst)
        == UNINITIALIZED
    {
        let f = OpenOptions::new()
            .write(true)
            .create(true)
            .open("/sys/kernel/debug/tracing/trace_marker")?;
        let trace_writer = TraceWriter { file: f };
        unsafe {
            GLOBAL_TRACE_WRITER = Some(trace_writer);
        }
        TRACE_WRITER_INIT.store(INITIALIZED, Ordering::SeqCst);
        Ok(())
    } else {
        Err(Error::new(ErrorKind::Other, "init trace writer fail!"))
    }
    #[cfg(not(unix))]
    Err(Error::new(
        io::ErrorKind::NotFound,
        "libatrace does not exist in this environment",
    ))
}

fn get_trace_writer() -> Option<&'static TraceWriter> {
    if TRACE_WRITER_INIT.load(Ordering::SeqCst) != INITIALIZED {
        return None;
    }
    unsafe {
        // This is safe given the invariant that setting the init trace writer
        // also sets `TRACE_WRITER_INIT` to `INITIALIZED`.
        Some(GLOBAL_TRACE_WRITER.as_ref().expect(
            "invariant violated: GLOBAL_TRACE_WRITER must be initialized before GLOBAL_TRACE_WRITER is set",
        ))
    }
}

pub fn trace_begin(name: &str) -> Result<(), Error> {
    #[cfg(unix)]
    init_trace_writer()?;
    if let Some(writer) = get_trace_writer() {
        // println!("writer:{:p}, file::{:?}", writer, writer.file);
        let mut w = &writer.file;
        let mut s = String::new();
        let _ = write!(&mut s, "B|{}|{}", process::id(), name);
        w.write(s.as_bytes())?;
        w.flush()?;
    }
    Ok(())
}

pub fn trace_end() -> Result<(), Error> {
    #[cfg(unix)]
    init_trace_writer()?;
    if let Some(writer) = get_trace_writer() {
        // println!("writer:{:p}, file::{:?}", writer, writer.file);
        let mut w = &writer.file;
        w.write_all(b"E")?;
        w.flush()?
    }
    Ok(())
}

#[derive(Default)]
pub struct ScopedTrace(u64);

impl ScopedTrace {
    pub fn new(tag: u64, name: &str) -> ScopedTrace {
        let _ = trace_begin(name);
        ScopedTrace(tag)
    }
}

impl Drop for ScopedTrace {
    fn drop(&mut self) {
        let _ = trace_end();
    }
}

#[macro_export]
macro_rules! TRACE_NAME {
    ($msg:expr) => {
        let st = ScopedTrace::new(0, $msg);
    };
    ($msg:expr,) => {
        let st = ScopedTrace::new(0, $msg);
    };
}

#[macro_export]
macro_rules! TRACE_NAME2 {
    ($msg:expr) => {
        let st = ScopedTrace::new(0, $msg);
    };
    ($msg:expr,) => {
        let st = ScopedTrace::new(0, $msg);
    };
    ($fmt:expr, $($arg:tt)+) => {
        let st = ScopedTrace::new(0, &std::fmt::format(format_args!($fmt, $($arg)+)));
    };
}

#[macro_export]
macro_rules! TRACE_BEGIN {
    ($msg:expr) => {
        let _ = trace_begin($msg);
    };
    ($msg:expr,) => {
        let _ = trace_begin($msg);
    };
    ($fmt:expr, $($arg:tt)+) => {
        let _ = trace_begin(&std::fmt::format(format_args!($fmt, $($arg)+)));
    };
}

#[macro_export]
macro_rules! TRACE_END {
    () => {
        let _ = trace_end();
    };
    ($($arg:tt)*) => {
        let _ = trace_end();
    };
}
