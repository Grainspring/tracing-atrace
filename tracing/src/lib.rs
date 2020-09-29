//! # tracing-atrace
//!
//! Support for logging [`tracing`][tracing] events natively to [linux kernel debug tracing].
//!
//! ## Overview
//!
//! [`tracing`] is a framework for instrumenting Rust programs to collect
//! scoped, structured, and async-aware diagnostics. `tracing-atrace` provides a
//! [`tracing-subscriber::Layer`][layer] implementation for logging `tracing` spans
//! and events to [`linux kernel debug tracing`][kernel debug tracing], on Linux distributions that
//! use `debugfs`.
//!  
//! *Compiler support: [requires `rustc` 1.40+][msrv]*
//!
//! [msrv]: #supported-rust-versions
//! [`tracing`]: https://crates.io/crates/tracing
//! [layer]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/layer/trait.Layer.html
//!
//! ## Supported Rust Versions
//!
//! Tracing is built against the latest stable release. The minimum supported
//! version is 1.40. The current Tracing version is not guaranteed to build on
//! Rust versions earlier than the minimum supported version.
//!
//! Tracing follows the same compiler support policies as the rest of the Tokio
//! project. The current stable Rust compiler and the three most recent minor
//! versions before it will always be supported. For example, if the current
//! stable compiler version is 1.45, the minimum supported version will not be
//! increased past 1.42, three minor versions prior. Increasing the minimum
//! supported compiler version is not considered a semver breaking change as
//! long as doing so complies with this policy.
//!
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/tokio-rs/tracing/master/assets/logo-type.png",
    issue_tracker_base_url = "https://github.com/tokio-rs/tracing/issues/"
)]
#[cfg(unix)]
use std::{fmt, fmt::Write, io};

use libatrace::{trace_begin, trace_end, TRACE_BEGIN, TRACE_END};
use tracing::{field, span, Span};
use tracing_core::{
    event::Event,
    field::Visit,
    span::{Attributes, Id, Record},
    Field, Subscriber,
};
use tracing_futures::{Instrument, Instrumented};
use tracing_subscriber::{layer::Context, registry::LookupSpan};

pub struct Layer {
    #[cfg(unix)]
    futobj_field: Option<String>,
    msg_field: Option<String>,
    data_field: Option<String>,
}

impl Layer {
    /// Construct a atrace layer
    ///
    pub fn new() -> io::Result<Self> {
        #[cfg(unix)]
        {
            Ok(Self {
                futobj_field: Some("__fut".into()),
                msg_field: Some("message".into()),
                data_field: None,
            })
        }
        #[cfg(not(unix))]
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "atrace does not exist in this environment",
        ))
    }

    /// Sets the data field name to tracing data.
    /// Defaults to `None`.
    pub fn with_data_field(mut self, x: Option<String>) -> Self {
        self.data_field = x;
        self
    }
}

/// Construct a atrace layer
pub fn layer() -> io::Result<Layer> {
    Layer::new()
}

impl<S> tracing_subscriber::Layer<S> for Layer
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fn new_span(&self, attrs: &Attributes, id: &Id, ctx: Context<S>) {
        let span = ctx.span(id).expect("unknown span");
        let mut buf = String::new();
        write!(&mut buf, "{}", span.name()).unwrap();

        // for get __fut fied value
        let mut fut = String::new();
        attrs.record(&mut SpanVisitor {
            buf: &mut fut,
            futobj_field: self.futobj_field.as_ref().map(|x| &x[..]),
            data_field: None,
        });

        // for get data fied value
        let mut data = String::new();
        attrs.record(&mut SpanVisitor {
            buf: &mut data,
            futobj_field: None,
            data_field: self.data_field.as_ref().map(|x| &x[..]),
        });

        // total output str
        if fut.is_empty() {
            write!(&mut buf, ",id:{:?}", id.into_u64()).unwrap();
        } else {
            write!(&mut buf, ",fut:{}", fut).unwrap();
        }
        if !data.is_empty() {
            write!(&mut buf, ",data:{}", data).unwrap();
        }
        span.extensions_mut().insert(SpanFields(buf));
    }

    fn on_record(&self, id: &Id, values: &Record, ctx: Context<S>) {
        let span = ctx.span(id).expect("unknown span");
        let mut exts = span.extensions_mut();
        let old_buf = &mut exts.get_mut::<SpanFields>().expect("missing fields").0;

        // for get __fut fied value
        let mut fut = String::new();
        values.record(&mut SpanVisitor {
            buf: &mut fut,
            futobj_field: self.futobj_field.as_ref().map(|x| &x[..]),
            data_field: None,
        });
        // for get data fied value
        let mut data = String::new();
        values.record(&mut SpanVisitor {
            buf: &mut data,
            data_field: self.data_field.as_ref().map(|x| &x[..]),
            futobj_field: None,
        });

        // update total output str
        if !fut.is_empty() || !data.is_empty() {
            let mut buf = String::new();
            write!(&mut buf, "{}", span.name()).unwrap();

            if fut.is_empty() {
                write!(&mut buf, ",id:{:?}", id.into_u64()).unwrap();
            } else {
                write!(&mut buf, ",fut:{}", fut).unwrap();
            }
            if !data.is_empty() {
                write!(&mut buf, ",data:{}", data).unwrap();
            }
            if buf != old_buf.as_ref() {
                *old_buf = buf;
            }
        }
    }

    fn on_event(&self, event: &Event, _ctx: Context<S>) {
        let mut buf = String::new();
        // Record event fields
        event.record(&mut EventVisitor::new(
            &mut buf,
            self.msg_field.as_ref().map(|x| &x[..]),
        ));

        #[cfg(unix)]
        TRACE_BEGIN!("{:?}", &buf);
        TRACE_END!();
    }

    fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
        let first = ctx.span(id).expect("expected: span id exists in registry");
        let exts = first.extensions();
        let fields = exts.get::<SpanFields>().expect("missing fields");
        // println!("on_enter:{}", fields.0);
        #[cfg(unix)]
        TRACE_BEGIN!("{:?}", &fields.0);
    }

    fn on_exit(&self, _id: &span::Id, _ctx: Context<'_, S>) {
        #[cfg(unix)]
        TRACE_END!();
    }
}

struct SpanFields(String);

struct SpanVisitor<'a> {
    buf: &'a mut String,
    futobj_field: Option<&'a str>,
    data_field: Option<&'a str>,
}

impl Visit for SpanVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if let Some(futobj_field) = self.futobj_field {
            if futobj_field == field.name() {
                write!(self.buf, "{:?}", value).unwrap();
            }
        }
        if let Some(data_field) = self.data_field {
            if data_field == field.name() {
                write!(self.buf, "{:?}", value).unwrap();
            }
        }
    }
}

struct EventVisitor<'a> {
    buf: &'a mut String,
    msg_field: Option<&'a str>,
}

impl<'a> EventVisitor<'a> {
    fn new(buf: &'a mut String, msg_field: Option<&'a str>) -> Self {
        Self { buf, msg_field }
    }
}

impl Visit for EventVisitor<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if let Some(msg_field) = self.msg_field {
            if field.name() == msg_field {
                // message
                write!(self.buf, "{:?}", value).unwrap();
            }
        }
    }
}

pub trait InstrumentExt: Instrument {
    fn instrument(self, span: Span) -> Instrumented<Self>;
}

impl<T: Sized + Instrument> InstrumentExt for T
where
    T: Instrument + Sized,
{
    fn instrument(self, span: Span) -> Instrumented<Self> {
        let d = field::debug(&self as *const T);
        span.record("__fut", &d);
        T::instrument(self, span)
    }
}
