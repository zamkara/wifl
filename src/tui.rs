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
    stdout:          io::Stdout,
    tag:             String,
    pub selections:  Vec<(String, String)>,
    in_alt:          bool,
    working:         bool,   // true after enter_working_mode()
    log:             Vec<String>,
    progress_active: bool,
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
        Ok(Self {
            stdout, tag: tag.to_string(), selections: Vec::new(),
            in_alt: true, working: false, log: Vec::new(), progress_active: false,
        })
    }

    // ── shared helpers ────────────────────────────────────────────────────────

    fn w() -> usize { terminal::size().map(|(w, _)| w as usize).unwrap_or(80) }
    fn wh() -> (usize, usize) {
        let (w, h) = terminal::size().unwrap_or((80, 24));
        (w as usize, h as usize)
    }

    fn draw_sep(&mut self, row: u16) -> Result<()> {
        queue!(self.stdout, cursor::MoveTo(0, row),
               terminal::Clear(ClearType::CurrentLine),
               SetForegroundColor(Color::DarkGrey), Print("─".repeat(Self::w())), ResetColor)?;
        Ok(())
    }

    fn draw_footer(&mut self, hint: &str) -> Result<()> {
        let (_, h) = Self::wh();
        queue!(self.stdout,
               cursor::MoveTo(0, (h - 1) as u16),
               terminal::Clear(ClearType::CurrentLine),
               SetForegroundColor(Color::DarkGrey), Print(format!("  {}", hint)), ResetColor)?;
        Ok(())
    }

    // ── selection screen ──────────────────────────────────────────────────────

    fn draw_select_screen(&mut self, label: &str, items: &[String], sel: usize,
                          top: usize, max_vis: usize) -> Result<()> {
        let (_, h) = Self::wh();
        queue!(self.stdout, cursor::MoveTo(0, 0), terminal::Clear(ClearType::All),
               SetForegroundColor(Color::White), SetAttribute(Attribute::Bold),
               Print(format!("  wifl  {}  ·  Windows Image Fetch + Install", self.tag)),
               SetAttribute(Attribute::Reset), ResetColor)?;

        self.draw_sep(1)?;

        let mut row = 2u16;
        for (lbl, val) in &self.selections {
            queue!(self.stdout, cursor::MoveTo(0, row), terminal::Clear(ClearType::CurrentLine),
                   cursor::MoveTo(4, row),
                   SetForegroundColor(Color::DarkGrey), Print(format!("{:<24}", lbl)), ResetColor,
                   SetForegroundColor(Color::White), Print(val), ResetColor)?;
            row += 1;
        }
        if !self.selections.is_empty() {
            self.draw_sep(row)?;
            row += 1;
        }

        row += 1; // blank before prompt
        queue!(self.stdout, cursor::MoveTo(4, row),
               SetAttribute(Attribute::Bold), SetForegroundColor(Color::White),
               Print(label), SetAttribute(Attribute::Reset), ResetColor)?;
        row += 2;

        for i in 0..max_vis {
            let abs = top + i;
            queue!(self.stdout, cursor::MoveTo(0, row), terminal::Clear(ClearType::CurrentLine))?;
            if abs < items.len() {
                if abs == sel {
                    queue!(self.stdout, cursor::MoveTo(4, row),
                           SetForegroundColor(Color::Cyan), SetAttribute(Attribute::Bold),
                           Print(format!("▶  {}", items[abs])),
                           SetAttribute(Attribute::Reset), ResetColor)?;
                } else {
                    queue!(self.stdout, cursor::MoveTo(4, row),
                           SetForegroundColor(Color::DarkGrey),
                           Print(format!("   {}", items[abs])), ResetColor)?;
                }
            }
            row += 1;
        }

        if items.len() > max_vis {
            queue!(self.stdout, cursor::MoveTo(4, row), terminal::Clear(ClearType::CurrentLine),
                   SetForegroundColor(Color::DarkGrey),
                   Print(format!("  ···  {} / {}", sel + 1, items.len())), ResetColor)?;
        }

        self.draw_footer("↑↓  navigate     Enter  confirm     q  quit")?;

        // clear lines between items and footer
        let footer_row = (h - 1) as u16;
        let mut clear_row = row + 1;
        if items.len() > max_vis { clear_row += 1; }
        while clear_row < footer_row {
            queue!(self.stdout, cursor::MoveTo(0, clear_row), terminal::Clear(ClearType::CurrentLine))?;
            clear_row += 1;
        }

        self.stdout.flush()?;
        Ok(())
    }

    pub fn select(&mut self, label: &str, items: &[String]) -> Result<usize> {
        assert!(!items.is_empty());
        let (_, h) = Self::wh();
        // Reserve space for header, existing selections, sep, prompt, footer
        let overhead = 5 + self.selections.len();
        let max_vis = items.len().min(h.saturating_sub(overhead)).max(3);
        let mut sel = 0usize;
        let mut top = 0usize;

        loop {
            self.draw_select_screen(label, items, sel, top, max_vis)?;

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

    // ── working mode ──────────────────────────────────────────────────────────
    // Stays in alternate screen. Fixed header + scrolling log below.

    fn working_header_rows(&self) -> usize {
        // tag + sep + selections + sep + blank
        2 + self.selections.len() + 1 + 1
    }

    fn draw_working_header(&mut self) -> Result<()> {
        queue!(self.stdout, cursor::MoveTo(0, 0), terminal::Clear(ClearType::All),
               SetAttribute(Attribute::Bold),
               Print(format!("  wifl  {}", self.tag)),
               SetAttribute(Attribute::Reset))?;

        self.draw_sep(1)?;

        let mut row = 2u16;
        for (label, value) in &self.selections {
            let short = label.strip_prefix("Select ").unwrap_or(label);
            queue!(self.stdout, cursor::MoveTo(0, row), terminal::Clear(ClearType::CurrentLine),
                   SetForegroundColor(Color::DarkGrey),
                   Print(format!("  {:<26}", short)),
                   ResetColor,
                   SetForegroundColor(Color::White),
                   Print(value),
                   ResetColor)?;
            row += 1;
        }

        self.draw_sep(row)?;
        Ok(())
    }

    fn redraw_log(&mut self) -> Result<()> {
        let (_, h) = Self::wh();
        let first_log_row = self.working_header_rows() as u16;
        let avail = (h as u16).saturating_sub(first_log_row);
        let start = self.log.len().saturating_sub(avail as usize);

        let mut row = first_log_row;
        for line in &self.log[start..] {
            queue!(self.stdout, cursor::MoveTo(0, row),
                   terminal::Clear(ClearType::CurrentLine),
                   Print(line))?;
            row += 1;
        }
        while row < h as u16 {
            queue!(self.stdout, cursor::MoveTo(0, row), terminal::Clear(ClearType::CurrentLine))?;
            row += 1;
        }
        self.stdout.flush()?;
        Ok(())
    }

    pub fn enter_working_mode(&mut self) -> Result<()> {
        self.working = true;
        self.draw_working_header()?;
        self.redraw_log()
    }

    fn push_log(&mut self, line: String) {
        self.progress_active = false;
        self.log.push(line);
        let _ = self.redraw_log();
    }

    pub fn step(&mut self, msg: &str) {
        self.push_log(format!("  \x1b[33m·\x1b[0m  {}", msg));
    }

    pub fn step_ok(&mut self, msg: &str) {
        self.push_log(format!("  \x1b[32m✓\x1b[0m  {}", msg));
    }

    pub fn info(&mut self, msg: &str) {
        self.push_log(format!("  \x1b[90m·\x1b[0m  {}", msg));
    }

    pub fn progress(&mut self, done: u64, total: u64) {
        let (w, _) = Self::wh();
        let bar_w = w.saturating_sub(36).min(48).max(12);
        let filled = if total > 0 { done as usize * bar_w / total as usize } else { 0 };
        let pct    = if total > 0 { done * 100 / total } else { 0 };
        let line = format!(
            "  \x1b[36m·\x1b[0m  {}{} {:>3}%  {:.1} / {:.1} GiB",
            "\x1b[36m█\x1b[0m".repeat(filled),
            "\x1b[90m░\x1b[0m".repeat(bar_w - filled),
            pct,
            done  as f64 / 1_073_741_824.0,
            total as f64 / 1_073_741_824.0,
        );
        if self.progress_active {
            if let Some(last) = self.log.last_mut() { *last = line; }
        } else {
            self.log.push(line);
            self.progress_active = true;
        }
        let _ = self.redraw_log();
    }

    pub fn progress_done(&mut self) {
        self.progress_active = false;
    }

    /// Feed a line from a piped child process (e.g. wimlib) into the log.
    pub fn child_line(&mut self, raw: &str) {
        let s = raw.trim();
        if s.is_empty() { return; }
        if s.starts_with('[') {
            self.log.push(format!("  \x1b[90m·\x1b[0m  {}", s));
        } else if s.ends_with("done") {
            self.log.push(format!("  \x1b[32m✓\x1b[0m  {}", s));
        } else {
            self.log.push(format!("  \x1b[90m·\x1b[0m  {}", s));
        }
        let _ = self.redraw_log();
    }

    /// Show a selection menu overlaid on the working-mode log area.
    /// Used for in-flight selections (e.g. image index) after working mode starts.
    pub fn select_in_working(&mut self, label: &str, items: &[String]) -> Result<usize> {
        assert!(!items.is_empty());
        let (_, h) = Self::wh();
        let header_rows = self.working_header_rows();
        let overhead = header_rows + 4; // prompt + blank + footer
        let max_vis = items.len().min(h.saturating_sub(overhead)).max(3);
        let mut sel = 0usize;
        let mut top = 0usize;

        loop {
            self.draw_working_header()?;

            let mut row = (header_rows + 1) as u16;
            queue!(self.stdout, cursor::MoveTo(4, row),
                   SetAttribute(Attribute::Bold), SetForegroundColor(Color::White),
                   Print(label), SetAttribute(Attribute::Reset), ResetColor)?;
            row += 2;

            for i in 0..max_vis {
                let abs = top + i;
                queue!(self.stdout, cursor::MoveTo(0, row),
                       terminal::Clear(ClearType::CurrentLine))?;
                if abs < items.len() {
                    if abs == sel {
                        queue!(self.stdout, cursor::MoveTo(4, row),
                               SetForegroundColor(Color::Cyan), SetAttribute(Attribute::Bold),
                               Print(format!("▶  {}", items[abs])),
                               SetAttribute(Attribute::Reset), ResetColor)?;
                    } else {
                        queue!(self.stdout, cursor::MoveTo(4, row),
                               SetForegroundColor(Color::DarkGrey),
                               Print(format!("   {}", items[abs])), ResetColor)?;
                    }
                }
                row += 1;
            }

            if items.len() > max_vis {
                queue!(self.stdout, cursor::MoveTo(4, row),
                       terminal::Clear(ClearType::CurrentLine),
                       SetForegroundColor(Color::DarkGrey),
                       Print(format!("  ···  {} / {}", sel + 1, items.len())),
                       ResetColor)?;
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
        // Redraw working header (now includes the new selection) + log
        self.draw_working_header()?;
        self.redraw_log()?;
        Ok(sel)
    }
}
