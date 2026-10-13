//! Temporary probe: find which byte resets the cursor.
use terminal_core::engine::TerminalEngine;
use terminal_core::session::{PollResult, TerminalSession};

fn main() {
    let mut session = TerminalSession::new(24, 80);
    session.spawn_shell("/bin/bash").expect("spawn failed");
    std::thread::sleep(std::time::Duration::from_millis(800));

    let mut raw = Vec::new();
    loop {
        match session.poll_pty() {
            Ok(PollResult::Data(data)) => raw.extend_from_slice(&data),
            _ => break,
        }
    }

    let mut engine = TerminalEngine::new(24, 80);
    let mut prev = (0usize, 0usize);
    for (i, &b) in raw.iter().enumerate() {
        engine.input(&[b]);
        let s = engine.state();
        let cur = (s.cursor.row, s.cursor.col);
        if cur.0 != prev.0 || cur.1 < prev.1 {
            println!("byte {i:3} = 0x{b:02x}: cursor {prev:?} -> {cur:?}");
        }
        prev = cur;
    }
    let s = engine.state();
    println!("final cursor = ({},{})", s.cursor.row, s.cursor.col);
    println!(
        "grid rows={} cols={}",
        engine.buffer().active().grid.rows,
        engine.buffer().active().grid.cols
    );
}
