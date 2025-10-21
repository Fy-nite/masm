#[cfg(feature = "ratatui_debug")]
mod ratatui_impl {
    use super::*;
    use std::cell::Cell;
    use std::time::Duration;
    use crossterm::event::{self, Event, KeyCode, KeyEvent};
    use ratatui::backend::CrosstermBackend;
    use ratatui::Terminal;
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::{Block, Borders, Paragraph, Wrap, List, ListItem};
    use ratatui::text::{Span, Spans};
    use ratatui::style::{Color, Modifier, Style};

    pub struct RatatuiDebuggerInner {
        continue_mode: Cell<bool>,
        memory_offset: Cell<usize>,
        focused_pane: Cell<u8>,
    }

    impl RatatuiDebuggerInner {
        pub fn new() -> Self { Self { continue_mode: Cell::new(false), memory_offset: Cell::new(0), focused_pane: Cell::new(2) } }

        fn draw_ui<B: ratatui::backend::Backend>(&self, terminal: &mut Terminal<B>, masi: &crate::disassembler::MASIFile, state: &crate::interpreter::State, pc: usize, opcode: u8) -> Result<(), std::io::Error> {
            let size = terminal.size()?;
            terminal.draw(|f| {
                // paint a background so the whole terminal gets a uniform color
                let bg = Block::default().style(Style::default().bg(Color::Black));
                f.render_widget(bg, size);

                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints([Constraint::Length(3), Constraint::Min(4), Constraint::Length(1)].as_ref())
                    .split(size);

                let status = format!("PC={} (0x{:X})  OPCODE=0x{:02X}  RIP=0x{:X}", pc, pc, opcode, state.rip);
                let header = Paragraph::new(Spans::from(vec![Span::raw(status)]))
                    .block(Block::default().borders(Borders::ALL).title("Status"));
                f.render_widget(header, chunks[0]);

                // body split: left = registers+memory, right = disasm
                let body = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(35), Constraint::Percentage(65)].as_ref())
                    .split(chunks[1]);

                // Left column: registers (top) and memory (bottom)
                let left = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(12), Constraint::Min(4)].as_ref())
                    .split(body[0]);

                // registers: colored name and values (hex + dec)
                let rev = crate::register_map::RegisterMap::build_id_to_name();
                let mut ids: Vec<_> = rev.keys().copied().collect(); ids.sort();
                let mut reg_items: Vec<ListItem> = Vec::new();
                for id in ids.iter() {
                    let name = rev.get(id).unwrap();
                    let val = state.regs.get(id).copied().unwrap_or(0);
                    let name_span = Span::styled(format!("{:<4}", name), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                    let hex_span = Span::raw(format!(" 0x{:016X}", val));
                    let dec_span = Span::styled(format!(" {:>12}", val), Style::default().fg(Color::Green));
                    let line = Spans::from(vec![name_span, hex_span, dec_span]);
                    reg_items.push(ListItem::new(line));
                }
                let regs = List::new(reg_items).block(Block::default().borders(Borders::ALL).title("Registers"));
                f.render_widget(regs, left[0]);

                // memory pane: show bytes around PC as hex + ASCII
                let focused = self.focused_pane.get();
                let mem_block_style = if focused == 2 { Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD) } else { Style::default() };
                let mem_block = Block::default().borders(Borders::ALL).title("Memory (hex | ascii)").style(mem_block_style);
                let mem_lines: Vec<Spans> = {
                    let mem_lock = state.memory.lock().unwrap();
                    let mem_len = mem_lock.len();
                    // compute bytes_per_row and rows based on available pane size so lines don't overflow
                    let avail_w = left[1].width as usize;
                    let avail_h = left[1].height as usize;
                    // addr column estimate and separator width
                    let addr_len = 12usize; // e.g. "0x00000000: "
                    let sep = 3usize; // " | " and small spacing
                    let bytes_per_row = std::cmp::max(4usize, std::cmp::min(32usize, (avail_w.saturating_sub(addr_len + sep)) / 4));
                    let rows = std::cmp::max(3usize, std::cmp::min(16usize, avail_h));
                    let offset = self.memory_offset.get();
                    // clamp offset to valid range
                    let max_start = if mem_len > bytes_per_row * rows { mem_len - bytes_per_row * rows } else { 0 };
                    let start = std::cmp::min(offset, max_start);
                    let end = std::cmp::min(mem_len, start + bytes_per_row * rows);
                    let mut lines: Vec<Spans> = Vec::new();
                    let mut addr = start;
                    while addr < end {
                        let row_end = std::cmp::min(end, addr + bytes_per_row);
                        let mut hex_parts: Vec<Span> = Vec::new();
                        for i in addr..row_end {
                            let b = mem_lock[i];
                            let s = format!("{:02X}", b);
                            // highlight byte at PC
                            let styled = if i == pc { Span::styled(s, Style::default().fg(Color::Black).bg(Color::Yellow)) } else { Span::raw(s) };
                            // ensure fixed-width display by padding each byte to 2 chars and adding a space
                            hex_parts.push(styled);
                            hex_parts.push(Span::raw(" "));
                        }
                        // ascii representation
                        let mut ascii = String::new();
                        for i in addr..row_end {
                            let b = mem_lock[i];
                            let ch = if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' };
                            ascii.push(ch);
                        }
                        let addr_span = Span::styled(format!("0x{:08X}: ", addr), Style::default().fg(Color::LightBlue));
                        let mut row_spans = vec![addr_span];
                        row_spans.extend(hex_parts);
                        row_spans.push(Span::raw(" | "));
                        row_spans.push(Span::styled(ascii, Style::default().fg(Color::Magenta)));
                        lines.push(Spans::from(row_spans));
                        addr += bytes_per_row;
                    }
                    lines
                };
                let mem_para = Paragraph::new(mem_lines).block(mem_block).wrap(Wrap { trim: true });
                f.render_widget(mem_para, left[1]);

                // Right column: disassembly area with current PC highlighted
                let asm = crate::disassembler::disassemble(masi);
                let all_lines: Vec<String> = asm.lines().map(|l| l.to_string()).collect();
                let needle = format!("{:08X}:", pc);
                let mut lines_spans: Vec<Spans> = Vec::new();
                for l in all_lines.iter() {
                    if l.trim_start().starts_with(&needle) {
                        let s = format!("→ {}", l);
                        lines_spans.push(Spans::from(vec![Span::styled(s, Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD))]));
                    } else {
                        lines_spans.push(Spans::from(Span::raw(l.clone())));
                    }
                }
                let dis = Paragraph::new(lines_spans).block(Block::default().borders(Borders::ALL).title("Disasm")).wrap(Wrap { trim: false });
                f.render_widget(dis, body[1]);

                let help = Paragraph::new(Spans::from(vec![Span::raw("s: step  c: continue  q: quit  d: redraw  1:regs 2:mem 3:disasm 8/2/4/6: mem scroll")]))
                    .block(Block::default().borders(Borders::NONE));
                f.render_widget(help, chunks[2]);
            })?;
            Ok(())
        }
    }

    impl crate::interpreter::Debugger for RatatuiDebuggerInner {
        fn before_execute(&mut self, masi: &crate::disassembler::MASIFile, state: &crate::interpreter::State, pc: usize, opcode: u8) -> crate::interpreter::DebuggerControl {
            if self.continue_mode.get() { return crate::interpreter::DebuggerControl::Continue; }

            // Setup terminal
            let mut stdout = std::io::stdout();
            crossterm::terminal::enable_raw_mode().ok();
            let backend = CrosstermBackend::new(&mut stdout);
            let mut terminal = Terminal::new(backend).unwrap();

            // clear any previous output and draw initial UI
            let _ = terminal.clear();
            let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode);

            // event loop: wait for key
            loop {
                if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                    if let Ok(Event::Key(KeyEvent { code, .. })) = event::read() {
                        match code {
                            KeyCode::Char('s') => { // step
                                let _ = terminal.clear();
                                crossterm::terminal::disable_raw_mode().ok();
                                return crate::interpreter::DebuggerControl::Continue;
                            }
                            KeyCode::Char('c') => { self.continue_mode.set(true); let _ = terminal.clear(); crossterm::terminal::disable_raw_mode().ok(); return crate::interpreter::DebuggerControl::Continue; }
                            KeyCode::Char('q') => { let _ = terminal.clear(); crossterm::terminal::disable_raw_mode().ok(); return crate::interpreter::DebuggerControl::Exit; }
                            KeyCode::Char('d') => { let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode); }
                            KeyCode::Char('1') => { self.focused_pane.set(1); let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode); }
                            KeyCode::Char('2') => { self.focused_pane.set(2); let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode); }
                            KeyCode::Char('3') => { self.focused_pane.set(3); let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode); }
                            // memory scrolling when memory pane focused
                            KeyCode::Char('8') | KeyCode::Up => {
                                if self.focused_pane.get() == 2 {
                                    let offs = self.memory_offset.get();
                                    let new = offs.saturating_sub(16);
                                    self.memory_offset.set(new);
                                    let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode);
                                }
                            }
                            KeyCode::Down => {
                                if self.focused_pane.get() == 2 {
                                    let offs = self.memory_offset.get();
                                    self.memory_offset.set(offs + 16);
                                    let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode);
                                }
                            }
                            KeyCode::Char('4') | KeyCode::Left => {
                                if self.focused_pane.get() == 2 {
                                    let offs = self.memory_offset.get();
                                    let new = offs.saturating_sub(1);
                                    self.memory_offset.set(new);
                                    let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode);
                                }
                            }
                            KeyCode::Char('6') | KeyCode::Right => {
                                if self.focused_pane.get() == 2 {
                                    let offs = self.memory_offset.get();
                                    self.memory_offset.set(offs + 1);
                                    let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode);
                                }
                            }
                            KeyCode::PageUp => {
                                if self.focused_pane.get() == 2 {
                                    let offs = self.memory_offset.get();
                                    let new = offs.saturating_sub(16*8);
                                    self.memory_offset.set(new);
                                    let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode);
                                }
                            }
                            KeyCode::PageDown => {
                                if self.focused_pane.get() == 2 {
                                    let offs = self.memory_offset.get();
                                    self.memory_offset.set(offs + 16*8);
                                    let _ = self.draw_ui(&mut terminal, masi, state, pc, opcode);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

#[cfg(feature = "ratatui_debug")]
pub use ratatui_impl::RatatuiDebuggerInner as RatatuiDebugger;

// If feature not enabled, provide a stub to avoid compile errors when the module is referenced
#[cfg(not(feature = "ratatui_debug"))]
pub struct RatatuiDebugger { }

#[cfg(not(feature = "ratatui_debug"))]
impl RatatuiDebugger { pub fn new() -> Self { Self { } } }


