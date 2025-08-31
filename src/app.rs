use std::env;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{poll, read, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, ClearType, EnterAlternateScreen, LeaveAlternateScreen,
};

use crate::editor::Editor;
use crate::keymap::{Action, Mode};
use crate::ui::Ui;

pub fn run() -> io::Result<()> {
    let mut ed = Editor::new()?;
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        let p = PathBuf::from(&args[1]);
        if let Err(e) = ed.open(p) {
            eprintln!("Failed to open file: {}", e);
        }
    }

    let mut ui = Ui::new()?;
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        crossterm::terminal::Clear(ClearType::All)
    )?;
    let res = (|| -> io::Result<()> {
        let mut needs_redraw = true;
        loop {
            if needs_redraw {
                ui.refresh(&mut stdout, &mut ed)?;
                needs_redraw = false;
            }
            // Compute dynamic timeout for status expiry to avoid spurious redraws
            let timeout = ui
                .time_until_status_expiry(&ed)
                .unwrap_or(Duration::from_millis(1_000_000));
            if poll(timeout)? {
                match read()? {
                    Event::Key(KeyEvent {
                        code, modifiers, ..
                    }) => match (code, modifiers) {
                        (KeyCode::Char('q'), KeyModifiers::CONTROL) => {
                            if ed.dirty && ed.quit_times > 0 {
                                ed.set_status("File modified — press Ctrl-Q again to quit");
                                ed.quit_times -= 1;
                                needs_redraw = true;
                            } else {
                                break;
                            }
                        }
                        (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                            if ed.filename.is_none() {
                                if let Ok(Some(name)) =
                                    ui.prompt_filename(&mut ed, &mut stdout, "Save as: ")
                                {
                                    ed.filename = Some(PathBuf::from(name));
                                    needs_redraw = true;
                                } else {
                                    ed.set_status("Save canceled");
                                    needs_redraw = true;
                                }
                            }
                            if ed.filename.is_some() {
                                if let Err(e) = ed.save() {
                                    ed.set_status(&format!("Save error: {}", e));
                                }
                                needs_redraw = true;
                            }
                        }
                        _ => match ed.mode {
                            Mode::Insert => match code {
                                KeyCode::Esc => {
                                    ed.mode = Mode::Normal;
                                    needs_redraw = true;
                                }
                                KeyCode::Enter => {
                                    ed.insert_newline();
                                    needs_redraw = true;
                                }
                                KeyCode::Backspace | KeyCode::Delete => {
                                    ed.delete_char();
                                    needs_redraw = true;
                                }
                                KeyCode::Up => {
                                    ed.apply_action(Action::MoveUp);
                                    needs_redraw = true;
                                }
                                KeyCode::Down => {
                                    ed.apply_action(Action::MoveDown);
                                    needs_redraw = true;
                                }
                                KeyCode::Left => {
                                    ed.apply_action(Action::MoveLeft);
                                    needs_redraw = true;
                                }
                                KeyCode::Right => {
                                    ed.apply_action(Action::MoveRight);
                                    needs_redraw = true;
                                }
                                KeyCode::Home => {
                                    ed.apply_action(Action::LineStart);
                                    needs_redraw = true;
                                }
                                KeyCode::End => {
                                    ed.apply_action(Action::LineEnd);
                                    needs_redraw = true;
                                }
                                KeyCode::PageUp => {
                                    ed.cy = ed.cy.saturating_sub(ui.screen_rows as usize);
                                    needs_redraw = true;
                                }
                                KeyCode::PageDown => {
                                    ed.cy = (ed.cy + ui.screen_rows as usize)
                                        .min(ed.buf.rows.len().saturating_sub(1));
                                    needs_redraw = true;
                                }
                                KeyCode::Char(c) => {
                                    if !modifiers.contains(KeyModifiers::CONTROL) && !c.is_control()
                                    {
                                        ed.insert_char(c);
                                        needs_redraw = true;
                                    }
                                }
                                _ => {}
                            },
                            Mode::Normal => match code {
                                KeyCode::Up => {
                                    ed.apply_action(Action::MoveUp);
                                    needs_redraw = true;
                                }
                                KeyCode::Down => {
                                    ed.apply_action(Action::MoveDown);
                                    needs_redraw = true;
                                }
                                KeyCode::Left => {
                                    ed.apply_action(Action::MoveLeft);
                                    needs_redraw = true;
                                }
                                KeyCode::Right => {
                                    ed.apply_action(Action::MoveRight);
                                    needs_redraw = true;
                                }
                                KeyCode::Home => {
                                    ed.apply_action(Action::LineStart);
                                    needs_redraw = true;
                                }
                                KeyCode::End => {
                                    ed.apply_action(Action::LineEnd);
                                    needs_redraw = true;
                                }
                                KeyCode::PageUp => {
                                    ed.cy = ed.cy.saturating_sub(ui.screen_rows as usize);
                                    needs_redraw = true;
                                }
                                KeyCode::PageDown => {
                                    ed.cy = (ed.cy + ui.screen_rows as usize)
                                        .min(ed.buf.rows.len().saturating_sub(1));
                                    needs_redraw = true;
                                }
                                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                                    if let Some(p) = ed.pending.take() {
                                        let seq = format!("{}{}", p, c);
                                        if let Some(&act) = ed.keymap.get(&seq) {
                                            if matches!(act, Action::CommandPrompt) {
                                                if let Ok(Some(cmd)) =
                                                    ui.prompt_command(&mut ed, &mut stdout)
                                                {
                                                    match cmd.as_str() {
                                                        "w" => {
                                                            let _ = ed.save();
                                                            needs_redraw = true;
                                                        }
                                                        "q" => {
                                                            break;
                                                        }
                                                        "wq" | "x" => {
                                                            let _ = ed.save();
                                                            break;
                                                        }
                                                        _ => {
                                                            ed.set_status("Unknown command");
                                                            needs_redraw = true;
                                                        }
                                                    }
                                                }
                                            } else {
                                                ed.apply_action(act);
                                                needs_redraw = true;
                                            }
                                        } else {
                                            let prev = p.to_string();
                                            if let Some(&act) = ed.keymap.get(&prev) {
                                                ed.apply_action(act);
                                                needs_redraw = true;
                                            }
                                            let cur = c.to_string();
                                            if (c == 'g' && ed.keymap.contains_key("gg"))
                                                || (c == 'd' && ed.keymap.contains_key("dd"))
                                            {
                                                ed.pending = Some(c);
                                            } else if let Some(&act) = ed.keymap.get(&cur) {
                                                if matches!(act, Action::CommandPrompt) {
                                                    if let Ok(Some(cmd)) =
                                                        ui.prompt_command(&mut ed, &mut stdout)
                                                    {
                                                        match cmd.as_str() {
                                                            "w" => {
                                                                let _ = ed.save();
                                                                needs_redraw = true;
                                                            }
                                                            "q" => {
                                                                break;
                                                            }
                                                            "wq" | "x" => {
                                                                let _ = ed.save();
                                                                break;
                                                            }
                                                            _ => {
                                                                ed.set_status("Unknown command");
                                                                needs_redraw = true;
                                                            }
                                                        }
                                                    }
                                                } else {
                                                    ed.apply_action(act);
                                                    needs_redraw = true;
                                                }
                                            }
                                        }
                                    } else {
                                        let s = c.to_string();
                                        if (c == 'g' && ed.keymap.contains_key("gg"))
                                            || (c == 'd' && ed.keymap.contains_key("dd"))
                                        {
                                            ed.pending = Some(c);
                                        } else if let Some(&act) = ed.keymap.get(&s) {
                                            if matches!(act, Action::CommandPrompt) {
                                                if let Ok(Some(cmd)) =
                                                    ui.prompt_command(&mut ed, &mut stdout)
                                                {
                                                    match cmd.as_str() {
                                                        "w" => {
                                                            let _ = ed.save();
                                                            needs_redraw = true;
                                                        }
                                                        "q" => {
                                                            break;
                                                        }
                                                        "wq" | "x" => {
                                                            let _ = ed.save();
                                                            break;
                                                        }
                                                        _ => {
                                                            ed.set_status("Unknown command");
                                                            needs_redraw = true;
                                                        }
                                                    }
                                                }
                                            } else {
                                                ed.apply_action(act);
                                                needs_redraw = true;
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            },
                        },
                    },
                    Event::Resize(w, h) => {
                        ui.resize(w, h);
                        ui.clear_cache();
                        needs_redraw = true;
                    }
                    _ => {}
                }
            } else {
                // Poll timeout: status may have expired
                if ui
                    .time_until_status_expiry(&ed)
                    .map(|d| d.is_zero())
                    .unwrap_or(false)
                {
                    ui.clear_cache();
                    needs_redraw = true;
                }
            }
        }
        Ok(())
    })();

    disable_raw_mode().ok();
    execute!(
        stdout,
        LeaveAlternateScreen,
        DisableMouseCapture,
        crossterm::cursor::Show,
        crossterm::terminal::Clear(ClearType::All),
        crossterm::cursor::MoveTo(0, 0)
    )
    .ok();
    res
}
