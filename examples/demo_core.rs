//! CLI test program for the terminal core.
//!
//! Demonstrates:
//! - Writing text
//! - SGR colors
//! - Cursor movement
//! - Line feed / wrapping
//! - Alternate screen
//! - Bracketed paste mode
//! - Hyperlinks (OSC 8)

use terminal_core::engine::TerminalEngine;
use terminal_core::session::TerminalSession;

#[expect(
    clippy::too_many_lines,
    reason = "Linear demo script exercising the core API"
)]
fn main() {
    println!("=== Terminal Core Demo ===\n");

    // 1. Basic text
    println!("1. Escribir texto básico:");
    let mut engine = TerminalEngine::new(24, 80);
    engine.input(b"Hello, World!");
    print_grid(&engine, 3, 20);
    println!();

    // 2. SGR colors
    println!("2. Colores SGR (\\x1b[31mRed\\x1b[32mGreen\\x1b[34mBlue):");
    let mut engine = TerminalEngine::new(24, 80);
    engine.input(b"Normal \x1b[31mRed \x1b[32mGreen \x1b[34mBlue");
    print_grid(&engine, 1, 40);
    println!();

    // 3. 256-color
    println!("3. Color 256 (\\x1b[38;5;208mOrange):");
    let mut engine = TerminalEngine::new(24, 80);
    engine.input(b"Normal \x1b[38;5;208mOrange");
    print_grid(&engine, 1, 25);
    println!();

    // 4. RGB color
    println!("4. Color RGB 24-bit (\\x1b[38;2;100;200;255mCyan):");
    let mut engine = TerminalEngine::new(24, 80);
    engine.input(b"Normal \x1b[38;2;100;200;255mCyan");
    print_grid(&engine, 1, 25);
    println!();

    // 5. Text attributes
    println!("5. Atributos (\\x1b[1mbold\\x1b[3mitalic\\x1b[4munderline):");
    let mut engine = TerminalEngine::new(24, 80);
    engine.input(b"Normal \x1b[1mbold \x1b[3mitalic \x1b[4munderline");
    print_grid(&engine, 1, 45);
    println!();

    // 6. Cursor movement
    println!("6. Mover cursor (\\x1b[5;10H):");
    let mut engine = TerminalEngine::new(24, 80);
    engine.input(b"Position: \x1b[5;10Hhere");
    print_grid(&engine, 8, 20);
    println!(
        "   Cursor en fila {}, col {}",
        engine.state().cursor.row,
        engine.state().cursor.col
    );
    println!();

    // 7. Line feed
    println!("7. Line feed (\\n):");
    let mut engine = TerminalEngine::new(24, 80);
    engine.input(b"Line 1\nLine 2\nLine 3");
    print_grid(&engine, 5, 15);
    println!();

    // 8. Line wrapping
    println!("8. Wrap around (grid 5 cols):");
    let mut engine = TerminalEngine::new(3, 5);
    engine.input(b"ABCDE"); // wraps to next line
    engine.input(b"FGHIJ");
    print_grid(&engine, 3, 5);
    println!(
        "   Cursor: ({}, {})",
        engine.state().cursor.row,
        engine.state().cursor.col
    );
    println!();

    // 9. Alternate screen
    println!("9. Alternate screen (\\x1b[?1049h):");
    let mut engine = TerminalEngine::new(5, 20);
    engine.input(b"Main screen content");
    println!("   Main antes de alt screen:");
    print_grid(&engine, 5, 20);
    engine.input(b"\x1b[?1049h");
    engine.input(b"Alt screen!");
    println!("   Alternate screen activo:");
    print_grid(&engine, 5, 20);
    engine.input(b"\x1b[?1049l");
    println!("   Main después de alt screen (contenido preservado):");
    print_grid(&engine, 5, 20);
    println!();

    // 10. Bracketed paste
    println!("10. Bracketed paste (\\x1b[?2004h):");
    let mut engine = TerminalEngine::new(24, 80);
    engine.input(b"Before \x1b[?2004h after");
    println!(
        "   Bracketed paste enabled: {}",
        engine.state().modes.bracketed_paste
    );
    println!();

    // 11. OSC 8 hyperlink
    println!("11. Hyperlink OSC-8:");
    let mut parser = terminal_core::parser::Parser::new();
    let actions = parser.parse_bytes(b"\x1b]8;;https://example.com\x07Click me\x1b]8;;\x07");
    println!("   Acciones parser:");
    for action in &actions {
        match action {
            terminal_core::parser::Action::HyperlinkStart(url) => {
                println!("      → HyperlinkStart({url})");
            }
            terminal_core::parser::Action::HyperlinkEnd => {
                println!("      → HyperlinkEnd");
            }
            terminal_core::parser::Action::Write(c) => {
                println!("      → Write('{c}')");
            }
            _ => {
                println!("      → {action:?}");
            }
        }
    }
    println!();

    // 12. Session
    println!("12. TerminalSession:");
    let mut session = TerminalSession::new(24, 80);
    session.feed(b"Session text\nSecond line");
    println!("   Running: {}", session.is_running());
    println!("   Buffer:");
    print_grid(session.engine(), 3, 20);
    session.stop();
    println!("   After stop: running = {}", session.is_running());
    session.feed(b"Should be ignored");
    println!(
        "   After feeding stopped session, cell(0,0) = '{}'",
        session.buffer().active().grid.get(0, 0).c
    );
    println!();

    // 13. Invalid bytes (CL-2)
    println!("13. Bytes inválidos (CL-2 — reemplazo con U+FFFD):");
    let mut engine = TerminalEngine::new(24, 80);
    engine.input(&[0x41, 0xff, 0xfe, 0x42]); // 'A', invalid, invalid, 'B'
    print_grid(&engine, 1, 5);
    println!();

    println!("=== Demo completada ===");
}

fn print_grid(engine: &TerminalEngine, rows: usize, cols: usize) {
    let buf = engine.buffer().active();
    for row in 0..rows.min(buf.grid.rows) {
        let mut line = String::new();
        for col in 0..cols.min(buf.grid.cols) {
            let cell = buf.grid.get(row, col);
            if cell.c == ' ' {
                line.push('·'); // show spaces as dots for visibility
            } else {
                line.push(cell.c);
            }
        }
        println!("   {row:2}: {line}");
    }
}
