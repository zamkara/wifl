use std::io::{self, Write};
use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, ClearType},
};

struct RawGuard;
impl RawGuard {
    fn enable() -> Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}
impl Drop for RawGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

pub fn select(prompt: &str, items: &[String]) -> Result<usize> {
    assert!(!items.is_empty(), "select called with empty list");

    let mut stdout = io::stdout();
    let _raw = RawGuard::enable()?;

    let term_h   = terminal::size()?.1 as usize;
    let win_size = items.len().min(term_h.saturating_sub(6));
    let mut sel  = 0usize;
    let mut top  = 0usize;

    // Print prompt above the list (stays fixed)
    queue!(
        stdout,
        Print("\r\n"),
        SetAttribute(Attribute::Bold),
        SetForegroundColor(Color::White),
        Print(format!("  {}\r\n\r\n", prompt)),
        SetAttribute(Attribute::Reset),
        ResetColor,
    )?;
    stdout.flush()?;

    // Draw list for the first time
    draw_list(&mut stdout, items, sel, top, win_size, true)?;

    loop {
        match event::read()? {
            Event::Key(KeyEvent { code, modifiers, .. }) => match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if sel > 0 {
                        sel -= 1;
                        if sel < top { top = sel; }
                        draw_list(&mut stdout, items, sel, top, win_size, false)?;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if sel + 1 < items.len() {
                        sel += 1;
                        if sel >= top + win_size { top = sel + 1 - win_size; }
                        draw_list(&mut stdout, items, sel, top, win_size, false)?;
                    }
                }
                KeyCode::Enter => {
                    break;
                }
                KeyCode::Esc
                | KeyCode::Char('q')
                | KeyCode::Char('c')
                    if modifiers.contains(KeyModifiers::CONTROL) || code == KeyCode::Char('q') =>
                {
                    // Clear the list lines before bailing
                    clear_list(&mut stdout, win_size)?;
                    drop(_raw);
                    anyhow::bail!("aborted");
                }
                _ => {}
            },
            _ => {}
        }
    }

    clear_list(&mut stdout, win_size)?;
    drop(_raw);

    // Print confirmation line
    println!("  \x1b[36m·\x1b[0m  {}", items[sel]);

    Ok(sel)
}

fn draw_list(
    stdout:   &mut impl Write,
    items:    &[String],
    sel:      usize,
    top:      usize,
    win_size: usize,
    first:    bool,
) -> Result<()> {
    if !first {
        let back = win_size + if items.len() > win_size { 1 } else { 0 };
        queue!(stdout, cursor::MoveUp(back as u16))?;
    }

    for i in 0..win_size {
        let abs = top + i;
        queue!(stdout, cursor::MoveToColumn(0), terminal::Clear(ClearType::CurrentLine))?;
        if abs < items.len() {
            if abs == sel {
                queue!(
                    stdout,
                    SetForegroundColor(Color::Cyan),
                    Print(format!("  > {}\r\n", items[abs])),
                    ResetColor,
                )?;
            } else {
                queue!(
                    stdout,
                    SetForegroundColor(Color::DarkGrey),
                    Print(format!("    {}\r\n", items[abs])),
                    ResetColor,
                )?;
            }
        } else {
            queue!(stdout, Print("\r\n"))?;
        }
    }

    if items.len() > win_size {
        queue!(
            stdout,
            cursor::MoveToColumn(0),
            terminal::Clear(ClearType::CurrentLine),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("  {}/{}\r\n", sel + 1, items.len())),
            ResetColor,
        )?;
    }

    stdout.flush()?;
    Ok(())
}

fn clear_list(stdout: &mut impl Write, win_size: usize) -> Result<()> {
    queue!(stdout, cursor::MoveUp(win_size as u16))?;
    for _ in 0..win_size {
        queue!(stdout, cursor::MoveToColumn(0), terminal::Clear(ClearType::CurrentLine), cursor::MoveDown(1))?;
    }
    queue!(stdout, cursor::MoveUp(win_size as u16))?;
    stdout.flush()?;
    Ok(())
}
