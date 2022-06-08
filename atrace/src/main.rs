#[macro_use(crate_version, crate_authors)]
extern crate clap;
use libc::{
    access, c_int, c_void, close, creat, free, malloc, memset, open, read, sendfile, sigaction,
    sigfillset, siginfo_t, sigset_t, write, EINVAL, F_OK, O_RDWR, SIGHUP, SIGINT, SIGQUIT, SIGSYS,
    SIGTERM, STDOUT_FILENO, W_OK,
};
use libz_sys::{
    self, deflate, deflateEnd, deflateInit_, inflate, inflateEnd, inflateInit_, z_stream,
    z_streamp, zlibVersion, Z_DEFAULT_COMPRESSION, Z_FINISH, Z_NO_FLUSH, Z_OK,
};
use std::convert::TryInto;
use std::fmt::Write as FmtWrite;
use std::fs::OpenOptions;
use std::io::Read as IoRead;
use std::io::Write as IoWrite;
use std::io::{self};
use std::mem;
use std::os::raw::c_char;
use std::os::unix::io::IntoRawFd;
use std::process::exit;
use std::ptr::null_mut;
use std::string::String;
use std::thread;
use std::time::Duration;

//command-line parsing
mod cli;

use self::cli::{parse_options, Config};

const SYSTEM_KERNEL_DEBUG_TRACE: &str = "/sys/kernel/debug/tracing/";
const BUFFER_LEN: usize = 64 * 1024;
const FILE_LEN: usize = 64 * 1024 * 1024;
const MAX_FILE_PATH_LEN: usize = 256;

static mut G_TRACE_ABORTED: bool = false;

/// Wrapper to interpret syscall exit codes and provide a rustacean `io::Result`
pub struct SyscallReturnCode(pub c_int);

impl SyscallReturnCode {
    /// Returns the last OS error if value is -1 or Ok(value) otherwise.
    pub fn into_result(self) -> std::io::Result<c_int> {
        if self.0 == -1 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(self.0)
        }
    }

    /// Returns the last OS error if value is -1 or Ok(()) otherwise.
    pub fn into_empty_result(self) -> std::io::Result<()> {
        self.into_result().map(|_| ())
    }
}

/// Type that represents a signal handler function.
pub type SignalHandler =
    extern "C" fn(num: c_int, info: *mut siginfo_t, _unused: *mut c_void) -> ();

extern "C" {
    fn __libc_current_sigrtmin() -> c_int;
    fn __libc_current_sigrtmax() -> c_int;
}

fn validate_signal_num(num: c_int) -> io::Result<c_int> {
    if num >= SIGHUP && num <= SIGSYS {
        Ok(num)
    } else {
        Err(io::Error::from_raw_os_error(EINVAL))
    }
}

pub fn register_signal_handler(signum: c_int, handler: SignalHandler) -> Result<(), io::Error> {
    let num = validate_signal_num(signum)?;
    // Safe, because this is a POD struct.
    let mut sigact: sigaction = unsafe { mem::zeroed() };
    sigact.sa_flags = libc::SA_SIGINFO;
    sigact.sa_sigaction = handler as usize;

    // We set all the bits of sa_mask, so all signals are blocked on the current thread while the
    // SIGSYS handler is executing. Safe because the parameter is valid and we check the return
    // value.
    if unsafe { sigfillset(&mut sigact.sa_mask as *mut sigset_t) } < 0 {
        return Err(io::Error::last_os_error());
    }

    // Safe because the parameters are valid and we check the return value.
    unsafe { SyscallReturnCode(sigaction(num, &sigact, null_mut())).into_empty_result() }
}

pub fn register_sig_handler() -> Result<(), io::Error> {
    register_signal_handler(SIGHUP, sigsys_handler)?;
    register_signal_handler(SIGINT, sigsys_handler)?;
    register_signal_handler(SIGQUIT, sigsys_handler)?;
    register_signal_handler(SIGTERM, sigsys_handler)?;
    Ok(())
}

extern "C" fn sigsys_handler(_num: c_int, info: *mut siginfo_t, _unused: *mut c_void) {
    // Safe because we're just reading some fields from a supposedly valid argument.
    let _si_signo = unsafe { (*info).si_signo };
    let _si_code = unsafe { (*info).si_code };
    unsafe {
        G_TRACE_ABORTED = true;
    }
}

fn file_is_exist(filename: &str) -> bool {
    let ret = unsafe { access(filename.as_ptr() as *const c_char, F_OK) };
    return ret != -1;
}

fn file_is_writable(filename: &str) -> bool {
    let ret = unsafe { access(filename.as_ptr() as *const c_char, W_OK) };
    return ret != -1;
}

fn truncate_file(path: &str) -> bool {
    let trace_fd = unsafe { creat(path.as_ptr() as *const c_char, 0) };
    if trace_fd < 0 {
        return false;
    }
    unsafe { close(trace_fd) };
    return true;
}

// Enable or disable tgid in tracing output.
fn set_print_tgid_enable_if_present(enable: bool) -> bool {
    // If file not exists maybe kernel ftrace tgid patch is not added.
    // This case should still return true to enable tracing.
    if file_is_exist(&strcat_for_file_path("options/print-tgid")) {
        return set_kernel_option_enable(&strcat_for_file_path("options/print-tgid"), enable);
    }
    return true;
}

// Enable or disable kernel ftrace using global clock.
/*
trace_clock:
    Whenever an event is recorded into the ring buffer, a
    "timestamp" is added. This stamp comes from a specified
    clock. By default, ftrace uses the "local" clock. This
    clock is very fast and strictly per cpu, but on some
    systems it may not be monotonic with respect to other
    CPUs. In other words, the local clocks may not be in sync
    with local clocks on other CPUs.

    local: Default clock, but may not be in sync across CPUs

    global: This clock is in sync with all CPUs but may
      be a bit slower than the local clock.
 */
fn set_global_clock_enable(enable: bool) -> bool {
    let mode;
    if enable {
        mode = "global";
    } else {
        mode = "local";
    }
    // If trace clock mode is already set, then just return here.
    // Or the change of mode will reset trace output.
    if is_traceclock_mode(mode) {
        return true;
    }
    trace_write_string(&strcat_for_file_path("trace_clock"), mode)
}

fn is_traceclock_mode(mode: &str) -> bool {
    let filename = &strcat_for_file_path("buffer_size_kb");
    let fd = OpenOptions::new().read(true).write(false).open(filename);
    if fd.is_err() {
        println!("error opening:{:?}\n", filename);
        return false;
    } else {
        let mut contents = String::new();
        let r = fd.unwrap().read_to_string(&mut contents);
        if r.unwrap() > 0 {
            let f = contents.find("[");
            if f.is_none() {
                return false;
            }
            let mut index = f.unwrap();
            index += 1;
            let ff = contents[index..].find("]");
            if f.is_none() {
                return false;
            }
            let mut index_end = ff.unwrap();
            index_end -= 1;
            if contents[index..index_end] == *mode {
                return true;
            }
        } else {
            return false;
        }
    }
    true
}

// Stream trace to stdout.
fn stream_trace() {
    // TODO: support stream trace with trace_pipe.
}

/*
overwrite - This controls what happens when the trace buffer is
              full. If "1" (default), the oldest events are
              discarded and overwritten. If "0", then the newest
              events are discarded.
            (see per_cpu/cpu0/stats for overrun and dropped)
 */
fn set_trace_overwrite_enable(enable: bool) -> bool {
    return set_kernel_option_enable(&strcat_for_file_path("options/overwrite"), enable);
}

// Enable or disable tracing.
/*
tracing_on:

    This sets or displays whether writing to the trace
    ring buffer is enabled. Echo 0 into this file to disable
    the tracer or 1 to enable it. Note, this only disables
    writing to the ring buffer, the tracing overhead may
    still be occurring.

    The kernel function tracing_off() can be used within the
    kernel to disable writing to the ring buffer, which will
    set this file to "0". User space can re-enable tracing by
    echoing "1" into the file.

    Note, the function and event trigger "traceoff" will also
    set this file to zero and stop tracing. Which can also
    be re-enabled by user space using this file.
 */
fn set_tracing_enabled(enable: bool) -> bool {
    return set_kernel_option_enable(&strcat_for_file_path("tracing_on"), enable);
}

// Enable or disable record cmdline.
fn set_trace_recordcmd_enable(enable: bool) -> bool {
    return set_kernel_option_enable(&strcat_for_file_path("options/record-cmd"), enable);
}

// Clear trace output.
fn clear_trace() -> bool {
    return truncate_file(&strcat_for_file_path("trace\0"));
}

/*
buffer_size_kb:
    This sets or displays the number of kilobytes each CPU
    buffer holds. By default, the trace buffers are the same size
    for each CPU. The displayed number is the size of the
    CPU buffer and not total size of all buffers. The
    trace buffers are allocated in pages (blocks of memory
    that the kernel uses for allocation, usually 4 KB in size).
    If the last page allocated has room for more bytes
    than requested, the rest of the page will be used,
    making the actual allocation bigger than requested or shown.
    ( Note, the size may not be a multiple of the page size
      due to buffer management meta-data. )

    Buffer sizes for individual CPUs may vary
    (see "per_cpu/cpu0/buffer_size_kb" below), and if they do
    this file will show "X".
 */
fn set_trace_buffer_size(size: u32) -> bool {
    let mut str = String::new();
    let _ = write!(&mut str, "{}", size);
    trace_write_string(&strcat_for_file_path("buffer_size_kb"), &str)
}

// Disable all kernel trace events.
fn disable_kernel_trace_events(config: &Config) -> bool {
    let mut ret = true;
    // TODO: support categories
    // sched
    if file_is_writable(&strcat_for_file_path("events/sched/sched_switch/enable")) {
        ret &= set_kernel_option_enable(
            &strcat_for_file_path("events/sched/sched_switch/enable"),
            config.cpu_sched,
        );
    }
    if file_is_writable(&strcat_for_file_path("events/sched/sched_wakeup/enable")) {
        set_kernel_option_enable(
            &strcat_for_file_path("events/sched/sched_wakeup/enable"),
            config.cpu_sched,
        );
    }
    // workqueue for thread name
    if file_is_writable(&strcat_for_file_path("events/workqueuq/enable")) {
        set_kernel_option_enable(&strcat_for_file_path("events/workqueue/enable"), true);
    }
    // freq
    if file_is_writable(&strcat_for_file_path("events/power/cpu_frequency/enable")) {
        set_kernel_option_enable(
            &strcat_for_file_path("events/power/cpu_frequency/enable"),
            false,
        );
    }

    if file_is_writable(&strcat_for_file_path("events/power/clock_set_rate/enable")) {
        set_kernel_option_enable(
            &strcat_for_file_path("events/power/clock_set_rate/enable"),
            false,
        );
    }

    // idle
    if file_is_writable(&strcat_for_file_path("events/power/cpu_idle/enable")) {
        set_kernel_option_enable(&strcat_for_file_path("events/power/cpu_idle/enable"), false);
    }

    return ret;
}

fn verify_kernel_trace_funcs(_funcs: &str) -> bool {
    // TODO:verify funcs
    return true;
}

// Set kernel funcs to trace by a comma separated list.
// Default this is not available, must enable dynamic ftrace configed in kernel config.
// See https://www.kernel.org/doc/Documentation/trace/ftrace.txt dynamic ftrace.
fn set_kernel_trace_funcs(funcs: &str) -> bool {
    let mut ret = true;
    if funcs.is_empty() {
        if file_is_writable(&strcat_for_file_path("current_tracer")) {
            ret &= trace_write_string(&strcat_for_file_path("current_tracer"), "nop");
        }
        if file_is_writable(&strcat_for_file_path("set_ftrace_filter")) {
            // TODO: false
            // ret &= truncate_file(&strcat_for_file_path("set_ftrace_filter"));
        }
    } else {
        ret &= trace_write_string(&strcat_for_file_path("current_tracer"), "function_graph");
        ret &= set_kernel_option_enable(&strcat_for_file_path("options/funcgraph-abstime"), true);
        ret &= set_kernel_option_enable(&strcat_for_file_path("options/funcgraph-cpu"), true);
        ret &= set_kernel_option_enable(&strcat_for_file_path("options/funcgraph-proc"), true);
        ret &= set_kernel_option_enable(&strcat_for_file_path("options/funcgraph-flat"), true);
        ret &= truncate_file(&strcat_for_file_path("set_ftrace_filter"));
        if ret {
            ret &= verify_kernel_trace_funcs(funcs);
        }
    }
    return ret;
}

// Write the given string to the file.
fn write_string(filename: &str, str: &str) -> bool {
    let mut ret = true;
    // use libc.open filename should with \0.
    // let fd = unsafe { open(filename.as_ptr() as *const c_char, open_flags) };
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(filename);

    if f.is_err() {
        println!("error opening {:?}\n", filename);
        ret = false;
    } else {
        let r = f.unwrap().write(str.as_bytes());
        if r.unwrap() != str.len() {
            ret = false;
        }
    }
    ret
}

fn trace_write_string(filename: &str, str: &str) -> bool {
    write_string(filename, str)
}

fn write_clock_sync_marker() {
    // TODO:with real time
    trace_write_string(
        &strcat_for_file_path("trace_marker"),
        "trace_event_clock_sync: parent_ts=9000000\n",
    );
}

// Enable or disable certain kernel ftrace options by write 1 or 0 to the file.
// in /sys/kernel/debug/tracing/options.
fn set_kernel_option_enable(filename: &str, enable: bool) -> bool {
    match enable {
        true => trace_write_string(filename, &"1".to_string()),
        _ => trace_write_string(filename, &"0".to_string()),
    }
}

fn strcat_for_file_path(str: &str) -> String {
    let mut path = String::with_capacity(MAX_FILE_PATH_LEN);
    path.push_str(SYSTEM_KERNEL_DEBUG_TRACE);
    path.push_str(str);
    path
}

// Clean up trace settings.
fn cleanup_trace(config: &Config) {
    disable_kernel_trace_events(config);
    set_trace_recordcmd_enable(false);
    set_trace_overwrite_enable(true);
    set_trace_buffer_size(1);
    set_global_clock_enable(false);
    set_print_tgid_enable_if_present(false);
    set_kernel_trace_funcs("");
}

fn print_trace(config: &Config) -> i32 {
    let filename = &strcat_for_file_path("trace\0");
    let trace_fd = unsafe { open(filename.as_ptr() as *const c_char, O_RDWR) };
    if trace_fd < 0 {
        return -1;
    }

    let mut ret: i32 = 0;
    if config.compress {
        let mut refresh = Z_NO_FLUSH;
        let size = mem::size_of::<z_stream>().try_into().unwrap();
        let stream: z_streamp = unsafe { malloc(size) as *mut z_stream };
        unsafe {
            memset(stream as *mut c_void, 0, size);
        }
        ret = unsafe {
            deflateInit_(
                stream,
                Z_DEFAULT_COMPRESSION,
                zlibVersion(),
                mem::size_of::<z_stream>().try_into().unwrap(),
            )
        };
        if ret != Z_OK {
            unsafe {
                free(stream as *mut c_void);
                close(trace_fd);
            }
            return -1;
        }

        let pibuf = unsafe { malloc(BUFFER_LEN) as *mut u8 };
        if pibuf == null_mut() {
            if trace_fd >= 0 {
                unsafe {
                    free(stream as *mut c_void);
                    close(trace_fd);
                }
            }
            return -1;
        }
        let pobuf = unsafe { malloc(BUFFER_LEN) as *mut u8 };
        if pobuf == null_mut() {
            unsafe {
                free(pibuf as *mut c_void);
                free(stream as *mut c_void);
            }
            if trace_fd >= 0 {
                unsafe {
                    close(trace_fd);
                }
            }
            return -1;
        } else {
            unsafe {
                (*stream).next_out = pobuf;
                (*stream).avail_out = BUFFER_LEN.try_into().unwrap();
            }
        }
        unsafe {
            while Z_OK == ret {
                if (*stream).avail_in == 0 {
                    ret = read(trace_fd, pibuf as *mut c_void, BUFFER_LEN)
                        .try_into()
                        .unwrap();
                    if ret < 0 {
                        break;
                    } else if ret == 0 {
                        refresh = Z_FINISH;
                    } else {
                        (*stream).next_in = pibuf;
                        (*stream).avail_in = ret.try_into().unwrap();
                    }
                }

                if (*stream).avail_out == 0 {
                    ret = write(STDOUT_FILENO, pobuf as *mut c_void, BUFFER_LEN)
                        .try_into()
                        .unwrap();
                    if ret < BUFFER_LEN as i32 {
                        (*stream).avail_out = BUFFER_LEN.try_into().unwrap();
                        break;
                    }
                    (*stream).next_out = pobuf;
                    (*stream).avail_out = BUFFER_LEN.try_into().unwrap();
                }
                ret = deflate(stream, refresh);
            }

            if ((*stream).avail_out as usize) < BUFFER_LEN {
                ret = write(
                    STDOUT_FILENO,
                    pobuf as *mut c_void,
                    BUFFER_LEN - (*stream).avail_out as usize,
                )
                .try_into()
                .unwrap();
            }

            deflateEnd(stream);
            free(pibuf as *mut c_void);
            free(pobuf as *mut c_void);
            free(stream as *mut c_void);
        }
    } else {
        let mut byte = unsafe { sendfile(STDOUT_FILENO, trace_fd, null_mut(), FILE_LEN) };

        while byte > 0 {
            byte = unsafe { sendfile(STDOUT_FILENO, trace_fd, null_mut(), FILE_LEN) };
        }
    }

    if trace_fd >= 0 {
        unsafe { close(trace_fd) };
    }

    return ret;
}

fn uncompress_trace(config: &Config) -> i32 {
    let f = OpenOptions::new()
        .create(false)
        .read(true)
        .write(false)
        .open(&config.uncompress_file);
    let mut ret: i32;
    if !f.is_err() {
        let mut refresh = Z_NO_FLUSH;
        let size = mem::size_of::<z_stream>().try_into().unwrap();
        let stream: z_streamp = unsafe { malloc(size) as *mut z_stream };
        unsafe {
            memset(stream as *mut c_void, 0, size);
        }
        ret = unsafe {
            inflateInit_(
                stream,
                zlibVersion(),
                mem::size_of::<z_stream>().try_into().unwrap(),
            )
        };
        if ret != Z_OK {
            unsafe { free(stream as *mut c_void) };
            return -1;
        }
        let pibuf = unsafe { malloc(BUFFER_LEN) as *mut u8 };
        if pibuf == null_mut() {
            unsafe { free(stream as *mut c_void) };
            return -1;
        }
        let pobuf = unsafe { malloc(BUFFER_LEN) as *mut u8 };
        if pobuf == null_mut() {
            unsafe { free(pibuf as *mut c_void) };
            unsafe { free(stream as *mut c_void) };
            return -1;
        } else {
            unsafe {
                (*stream).next_out = pobuf;
                (*stream).avail_out = BUFFER_LEN.try_into().unwrap();
            }
        }

        let fd = f.unwrap().into_raw_fd();
        unsafe {
            while Z_OK == ret {
                if (*stream).avail_in == 0 {
                    ret = read(fd, pibuf as *mut c_void, BUFFER_LEN)
                        .try_into()
                        .unwrap();
                    if ret < 0 {
                        break;
                    } else if ret == 0 {
                        refresh = Z_FINISH;
                    } else {
                        (*stream).next_in = pibuf;
                        (*stream).avail_in = ret.try_into().unwrap();
                    }
                }

                if (*stream).avail_out == 0 {
                    ret = write(STDOUT_FILENO, pobuf as *mut c_void, BUFFER_LEN)
                        .try_into()
                        .unwrap();
                    if ret < BUFFER_LEN as i32 {
                        (*stream).avail_out = BUFFER_LEN.try_into().unwrap();
                        break;
                    }
                    (*stream).next_out = pobuf;
                    (*stream).avail_out = BUFFER_LEN.try_into().unwrap();
                }
                ret = inflate(stream, refresh);
            }

            if ((*stream).avail_out as usize) < BUFFER_LEN {
                ret = write(
                    STDOUT_FILENO,
                    pobuf as *mut c_void,
                    BUFFER_LEN - (*stream).avail_out as usize,
                )
                .try_into()
                .unwrap();
            }

            inflateEnd(stream);
            free(pibuf as *mut c_void);
            free(pobuf as *mut c_void);
            free(stream as *mut c_void);
        }
    } else {
        println!("open trace file:{:?} fail.\n", &config.uncompress_file);
        return -1;
    }
    return ret;
}

fn main() {
    let mut config = parse_options();
    // These are for async tracing.
    // Whether begin trace now.
    let mut begin = true;
    // Whether dump the trace result now.
    let mut dump = true;
    // Whether stop tracing now.
    let mut stop = true;
    let mut trace_async = false;

    // trace out to system logcat
    let mut trace_stream = false;

    if config.begin_async {
        trace_async = true;
        stop = false;
        dump = false;
        config.overwrite = true;
    }
    if config.stop_async {
        begin = false;
        trace_async = true;
    }
    if config.dump_async {
        begin = false;
        trace_async = true;
        stop = false;
    }
    if config.show_category {
        // list_supported_categories();
        println!("no support categories");
        exit(0);
    }
    if config.stream {
        trace_stream = true;
        dump = false;
    }
    // register sig handler for catch ctl+C/Z
    let _ = register_sig_handler();
    let mut ret = true;

    // check uncompress trace content in args.
    if !config.uncompress_file.is_empty() {
        let result = uncompress_trace(&config);
        exit(result);
    }

    // begin trace after sleep time
    if config.sleepsec > 0 {
        thread::sleep(Duration::from_millis((config.sleepsec * 1000).into()));
    }

    // prepare with setup trace
    ret &= setup_trace(&config);
    ret &= set_tracing_enabled(true);

    // begin trace within specified time
    if ret && begin {
        if !trace_stream {
            let _ = io::stdout().flush();
        }
        ret = clear_trace();
        write_clock_sync_marker();
        if ret && !trace_async && !trace_stream {
            thread::sleep(Duration::from_millis((config.durationsec * 1000).into()));
        }
        // TODO: support trace_stream
        if trace_stream {
            stream_trace();
        }
    }
    // end stop after specified time passed.
    if stop {
        set_tracing_enabled(false);
    }
    // dump trace event data.
    if ret && dump {
        if !unsafe { G_TRACE_ABORTED } {
            let _ = io::stdout().flush();
            print_trace(&config);
        } else {
            let _ = io::stdout().flush();
        }
        clear_trace();
    } else if !ret {
        println!("unable to start tracing, please check debugfs setup correctly\n");
    }

    if stop {
        cleanup_trace(&config);
    }
}

// Set up all kernel ftrace settings for this capture.
// Return true if all the settings are able to set.
fn setup_trace(config: &Config) -> bool {
    let mut ret = true;
    // Set if overwrite old trace if buffer is full.
    ret &= set_trace_overwrite_enable(config.overwrite);
    // Set traing buffer size.
    ret &= set_trace_buffer_size(config.buflen);

    // Enable global clock for tracing.
    ret &= set_global_clock_enable(true);

    // Set kernel tracers.
    ret &= set_kernel_trace_funcs(config.funcs.as_ref());

    // Enable tgid print in kernel ftrace if enabled.
    if config.tgid {
        ret &= set_print_tgid_enable_if_present(true);
    }

    // Enable recording cmdline of task when tracing.
    ret &= set_trace_recordcmd_enable(true);

    // Handles kernel trace events tags like "sched freq".
    // First, disable all the events.
    ret &= disable_kernel_trace_events(config);

    ret
}
