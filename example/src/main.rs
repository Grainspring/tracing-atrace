use libatrace::{trace_begin, trace_end, ScopedTrace, TRACE_NAME, TRACE_NAME2, TRACE_BEGIN, TRACE_END};

fn f1() {
    TRACE_BEGIN!("f1");
    let mut i = 0;
    TRACE_NAME2!("i:{}", i);
    println!("in f1 fn");
    i += 1;
    {
       TRACE_BEGIN!("f1 sub block");
       TRACE_END!();
    }
    println!("in f1 i:{}", i);
    {
        TRACE_NAME2!("i:{}", i);
    }
    TRACE_NAME!("trace end in f1");
    TRACE_END!();
}

fn main() {
    f1();
    TRACE_NAME!("trace end in main");
}
