use std::io::{self, Write};
use crossterm::{
    cursor, execute, queue,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use anyhow::Result;

// ─── Tui ─────────────────────────────────────────────────────────────────────

pub struct Tui {
    stdout:     io::Stdout,
    tag:        String,
    pub selections: Vec<(String, String)>,  // (label, chosen value)
    in_alt:     bool,
}

impl Drop for Tui {
    fn drop(&mut self) {
        if self.in_alt {
            let _ = execute!(self.stdout, LeaveAlternateScreen, cursor::Show);
            let _ = terminal::disable_raw_mode();
        }
    }
}

impl Tui {
    pub fn new(tag: &str) -> Result<Self> {
        let mut stdout = io::stdout();
        terminal::enable_raw_mode()?;
        execute!(stdout, EnterAlternateScreen, cursor::Hide)?;
        Ok(Self { stdout, tag: tag.to_string(), selections: Vec::new(), in_alt: true })
    }

    // ── low-level drawing ─────────────────────────────────────────────────────

    fn draw_header(&mut self) -> Result<u16> {
        let w = terminal::size().map(|(w, _)| w).unwrap_or(80) as usize;
        queue!(
            self.stdout,
            cursor::MoveTo(0, 0),
            terminal::Clear(ClearType::All),
            SetForegroundColor(Color::White),
            SetAttribute(Attribute::Bold),
            Print(format!("  wifl  {}  ·  Windows Image Fetch + Install", self.tag)),
            SetAttribute(Attribute::Reset),
            ResetColor,
            cursor::MoveTo(0, 1),
            SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(w)),
            ResetColor,
        )?;
        Ok(2)
    }

    fn draw_selections(&mut self, mut row: u16) -> Result<u16> {
        for (label, value) in &self.selections {
            queue!(
                self.stdout,
                cursor::MoveTo(0, row),
                terminal::Clear(ClearType::CurrentLine),
                cursor::MoveTo(4, row),
                SetForegroundColor(Color::DarkGrey),
                Print(format!("{:<24}", label)),
                ResetColor,
                SetForegroundColor(Color::White),
                Print(value),
                ResetColor,
            )?;
            row += 1;
        }
        if !self.selections.is_empty() { row += 1; }
        Ok(row)
    }

    fn draw_sep(&mut self, row: u16) -> Result<()> {
        let w = terminal::size().map(|(w, _)| w).unwrap_or(80) as usize;
        queue!(
            self.stdout,
            cursor::MoveTo(0, row),
            SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(w)),
            ResetColor,
        )?;
        Ok(())
    }

    fn draw_footer(&mut self, hint: &str) -> Result<()> {
        let h = terminal::size().map(|(_, h)| h).unwrap_or(24);
        queue!(
            self.stdout,
            cursor::MoveTo(0, h - 1),
            terminal::Clear(ClearType::CurrentLine),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("    {}", hint)),
            ResetColor,
        )?;
        Ok(())
    }

    // ── public: interactive menu ──────────────────────────────────────────────

    pub fn select(&mut self, label: &str, items: &[String]) -> Result<usize> {
        assert!(!items.is_empty());
        let (_, h) = terminal::size().unwrap_or((80, 24));
        let max_vis = items.len().min(h as usize / 3).max(3);
        let mut sel = 0usize;
        let mut top = 0usize;

        loop {
            let mut row = self.draw_header()?;
            row = self.draw_selections(row)?;

            if !self.selections.is_empty() {
                self.draw_sep(row)?;
                row += 1;
            }
            row += 1;

            // prompt
            queue!(
                self.stdout,
                cursor::MoveTo(4, row),
                SetAttribute(Attribute::Bold),
                SetForegroundColor(Color::White),
                Print(label),
                SetAttribute(Attribute::Reset),
                ResetColor,
            )?;
            row += 2;

            // items
            for i in 0..max_vis {
                let abs = top + i;
                queue!(self.stdout, cursor::MoveTo(0, row), terminal::Clear(ClearType::CurrentLine))?;
                if abs < items.len() {
                    if abs == sel {
                        queue!(
                            self.stdout,
                            cursor::MoveTo(4, row),
                            SetForegroundColor(Color::Cyan),
                            SetAttribute(Attribute::Bold),
                            Print(format!("▶  {}", items[abs])),
                            SetAttribute(Attribute::Reset),
                            ResetColor,
                        )?;
                    } else {
                        queue!(
                            self.stdout,
                            cursor::MoveTo(4, row),
                            SetForegroundColor(Color::DarkGrey),
                            Print(format!("   {}", items[abs])),
                            ResetColor,
                        )?;
                    }
                }
                row += 1;
            }

            if items.len() > max_vis {
                queue!(
                    self.stdout,
                    cursor::MoveTo(4, row),
                    terminal::Clear(ClearType::CurrentLine),
                    SetForegroundColor(Color::DarkGrey),
                    Print(format!("   ···  {} / {}", sel + 1, items.len())),
                    ResetColor,
                )?;
            }

            self.draw_footer("↑↓  navigate     Enter  confirm     q  quit")?;
            self.stdout.flush()?;

            match event::read()? {
                Event::Key(KeyEvent { code, modifiers, .. }) => match code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if sel > 0 { sel -= 1; if sel < top { top = sel; } }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if sel + 1 < items.len() {
                            sel += 1;
                            if sel >= top + max_vis { top = sel + 1 - max_vis; }
                        }
                    }
                    KeyCode::Enter => break,
                    KeyCode::Char('q') | KeyCode::Esc => anyhow::bail!("aborted"),
                    KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                        anyhow::bail!("aborted")
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        self.selections.push((label.to_string(), items[sel].clone()));
        Ok(sel)
    }

    // ── public: transition to working mode ───────────────────────────────────
    // Leaves alt screen and returns to normal terminal for the install phase.
    // Long operations (wimlib apply) write their own output here freely.

    pub fn enter_working_mode(&mut self) -> Result<()> {
        execute!(self.stdout, LeaveAlternateScreen, cursor::Show)?;
        self.in_alt = false;
        terminal::disable_raw_mode()?;

        let summary: String = self.selections.iter()
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
            .join("  ·  ");

        println!();
        println!("  \x1b[1mwifl\x1b[0m  \x1b[90m{}\x1b[0m", self.tag);
        println!("  \x1b[90m{}\x1b[0m", summary);
        println!();
        Ok(())
    }

    // ── public: step messages (used after enter_working_mode) ─────────────────

    pub fn step(&self, msg: &str) {
        println!("  \x1b[33m·\x1b[0m  {}", msg);
    }

    pub fn step_ok(&self, msg: &str) {
        println!("  \x1b[32m✓\x1b[0m  {}", msg);
    }

    pub fn info(&self, msg: &str) {
        println!("  \x1b[90m·\x1b[0m  {}", msg);
    }

    // Inline download progress bar — call repeatedly; ends with newline on done()
    pub fn progress(&self, done: u64, total: u64) {
        let w = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
        let bar_w = (w.saturating_sub(36)).min(48).max(12);
        let filled = if total > 0 { done as usize * bar_w / total as usize } else { 0 };
        let pct    = if total > 0 { done * 100 / total } else { 0 };
        print!(
            "\r  \x1b[36m·\x1b[0m  {}{} {:>3}%  {:.1} / {:.1} GiB",
            "\x1b[36m█\x1b[0m".repeat(filled),
            "\x1b[90m░\x1b[0m".repeat(bar_w - filled),
            pct,
            done  as f64 / 1_073_741_824.0,
            total as f64 / 1_073_741_824.0,
        );
        let _ = io::stdout().flush();
    }

    pub fn progress_done(&self) {
        println!(); // finish the \r line
    }
}
