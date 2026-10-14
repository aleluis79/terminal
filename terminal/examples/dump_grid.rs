//! Dump a captured byte stream through the terminal engine as plain text.
//!
//! Usage: cargo run -p terminal --example `dump_grid` -- <capture.bin> [rows cols]
//!
//! Feeds the file through `TerminalEngine` and prints the resulting grid, so the
//! visual result of escape sequences can be inspected without a display.

use std::io::Read;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: dump_grid <file> [rows cols]");
    let rows: usize = args.get(2).map_or(40, |v| v.parse().unwrap());
    let cols: usize = args.get(3).map_or(120, |v| v.parse().unwrap());
    let scroll: usize = args.get(4).map_or(0, |v| v.parse().unwrap());

    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .expect("open capture")
        .read_to_end(&mut bytes)
        .expect("read capture");

    let mut engine = terminal_core::engine::TerminalEngine::new(rows, cols);
    engine.input(&bytes);
    engine.buffer_mut().scroll_back(scroll);

    for row in engine.buffer().visible_rows() {
        let line: String = row.iter().map(|cell| cell.c).collect();
        println!("{}", line.trim_end());
    }
}
