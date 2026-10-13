#![allow(missing_docs)]
use terminal_core::parser::{Action, Parser};
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
    let pos = raw.iter().position(|&b| b==b'\r').unwrap();
    let mut parser = Parser::new();
    let actions = parser.parse_bytes(&raw[..pos]);
    println!("num actions first part: {}", actions.len());
    for (i,a) in actions.iter().enumerate() {
        match a {
            Action::Write(_) => {}
            other => println!("[{i}] {other:?}"),
        }
    }
}
