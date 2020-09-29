use clap::{App, Arg};

pub struct Config {
    pub debug_cmdline: String,
    pub overwrite: bool,
    pub buflen: u32,
    pub sleepsec: u32,
    pub durationsec: u32,
    pub compress: bool,
    pub uncompress_file: String,
    pub tgid: bool,
    pub begin_async: bool,
    pub stop_async: bool,
    pub dump_async: bool,
    pub show_category: bool,
    pub stream: bool,
    pub funcs: String,
    pub group: Vec<String>,
}

pub fn parse_options() -> Config {
    let cmd_arguments = App::new("atrace")
        .version(crate_version!())
        .author(crate_authors!())
        .about("Launch atrace.")
        .arg(
            Arg::with_name("A")
                .short("A")
                .help("the comma separate the cmdlines")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("B")
                .short("B")
                .help("the buffer size of the trace")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("C")
                .short("C")
                .help("use a cyclic queuetrace for trace")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("K")
                .short("K")
                .help("get trace of the kernel functions listed")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("S")
                .short("S")
                .help("trace after sleeping M seconds")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("T")
                .short("T")
                .help("trace duration M seconds")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("Z")
                .short("Z")
                .help("compress output trace with no plain text.")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("trace_file")
                .long("uncompress")
                .short("d")
                .multiple(true)
                .number_of_values(1)
                .help("uncompress trace file which maybe -Z trace output."),
        )
        .arg(
            Arg::with_name("G")
                .short("G")
                .help("disable tgid in trace output")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("BEGIN_ASYNC")
                .long("BEGIN_ASYNC")
                .help("begin trace and rapidly return")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("DUMP_ASYNC")
                .long("DUMP_ASYNC")
                .help("dump the trace buffer")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("STOP_ASYNC")
                .long("STOP_ASYNC")
                .help("stop tracing and rapidly dump buffer")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("SHOW_CATEGORY")
                .long("SHOW_CATEGORY")
                .help("show all the categories")
                .takes_value(false),
        )
        .arg(
            Arg::with_name("STREAM")
                .long("STREAM")
                .help("stream trace to stdout")
                .takes_value(false),
        )
        .arg(Arg::with_name("Group").multiple(true))
        .get_matches();

    let debug_cmdline = cmd_arguments.value_of("A").unwrap_or("").to_string();
    let buflen = cmd_arguments
        .value_of("B")
        .unwrap_or("1024")
        .parse::<u32>()
        .unwrap();

    let overwrite = cmd_arguments.is_present("C");
    let funcs = cmd_arguments.value_of("K").unwrap_or("").to_string();
    let sleepsec = cmd_arguments
        .value_of("S")
        .unwrap_or("0")
        .parse::<u32>()
        .unwrap();
    let durationsec = cmd_arguments
        .value_of("T")
        .unwrap_or("5")
        .parse::<u32>()
        .unwrap();

    let compress = cmd_arguments.is_present("Z");
    let uncompress_file = cmd_arguments
        .value_of("trace_file")
        .unwrap_or("")
        .to_string();
    let tgid = !cmd_arguments.is_present("G");

    let begin_async = cmd_arguments.is_present("BEGIN_ASYNC");
    let stop_async = cmd_arguments.is_present("STOP_ASYNC");
    let dump_async = cmd_arguments.is_present("DUMP_ASYNC");
    let show_category = cmd_arguments.is_present("SHOW_CATEGORY");
    let stream = cmd_arguments.is_present("STREAM");
    let _group = cmd_arguments
        .values_of("Group")
        .map(|vals| vals.collect::<Vec<_>>());

    Config {
        debug_cmdline,
        buflen,
        funcs,
        overwrite,
        sleepsec,
        durationsec,
        compress,
        uncompress_file,
        tgid,
        begin_async,
        stop_async,
        dump_async,
        show_category,
        stream,
        group: vec![],
    }
}
