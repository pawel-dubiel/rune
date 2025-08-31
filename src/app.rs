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
        let mut insert_undo_break_pending = false;
        let mut needs_redraw = true;
        loop {
            if needs_redraw {
                ui.refresh(&mut stdout, &mut ed)?;
                needs_redraw = false;
            }
            // Compute dynamic timeout for status expiry to avoid spurious redraws
            let mut timeout = Duration::from_millis(1_000_000);
            if let Some(t) = ui.time_until_status_expiry(&ed) {
                timeout = std::cmp::min(timeout, t);
            }
            // Sequence timeout for pending multi-key sequences
            const SEQ_TIMEOUT_MS: u64 = 1000;
            if let Some(t) = ed.time_until_pending_timeout(SEQ_TIMEOUT_MS) {
                timeout = std::cmp::min(timeout, t);
            }
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
                                    ed.end_undo_group();
                                    ed.mode = Mode::Normal;
                                    needs_redraw = true;
                                }
                                KeyCode::Char('z') if modifiers.contains(KeyModifiers::CONTROL) => {
                                    // Common muscle memory; Vim uses 'u' in normal, but support Ctrl-Z here in insert
                                    if ed.undo() {
                                        needs_redraw = true;
                                    }
                                }
                                KeyCode::Char('g') if modifiers.contains(KeyModifiers::CONTROL) => {
                                    // Start Ctrl-g sequence
                                    insert_undo_break_pending = true;
                                }
                                KeyCode::Char('u') if insert_undo_break_pending => {
                                    // Ctrl-g u: break undo group in insert mode
                                    ed.end_undo_group();
                                    insert_undo_break_pending = false;
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
                                    insert_undo_break_pending = false;
                                    if !modifiers.contains(KeyModifiers::CONTROL) && !c.is_control()
                                    {
                                        ed.insert_char(c);
                                        needs_redraw = true;
                                    }
                                }
                                _ => {}
                            },
                            Mode::Normal => match code {
                                KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
                                    if ed.redo() {
                                        needs_redraw = true;
                                    }
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
                                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                                    match ed.process_normal_char(c) {
                                        crate::editor::NormalInputResult::CommandPrompt => {
                                            if let Ok(Some(cmd)) =
                                                ui.prompt_command(&mut ed, &mut stdout)
                                            {
                                                if !ed.execute_ex_command(&cmd) {
                                                    match cmd.as_str() {
                                                        "w" => {
                                                            let _ = ed.save();
                                                            needs_redraw = true;
                                                        }
                                                        "q" => break,
                                                        "wq" | "x" => {
                                                            let _ = ed.save();
                                                            break;
                                                        }
                                                        _ => {
                                                            ed.set_status("Unknown command");
                                                            needs_redraw = true;
                                                        }
                                                    }
                                                } else {
                                                    needs_redraw = true;
                                                }
                                            }
                                        }
                                        crate::editor::NormalInputResult::None => {
                                            needs_redraw = true;
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
                // Pending sequence may have timed out
                if ed
                    .time_until_pending_timeout(SEQ_TIMEOUT_MS)
                    .map(|d| d.is_zero())
                    .unwrap_or(false)
                {
                    match ed.process_pending_timeout() {
                        crate::editor::NormalInputResult::CommandPrompt => {
                            if let Ok(Some(cmd)) = ui.prompt_command(&mut ed, &mut stdout) {
                                if !ed.execute_ex_command(&cmd) {
                                    match cmd.as_str() {
                                        "w" => {
                                            let _ = ed.save();
                                            needs_redraw = true;
                                        }
                                        "q" => break,
                                        "wq" | "x" => {
                                            let _ = ed.save();
                                            break;
                                        }
                                        _ => {
                                            ed.set_status("Unknown command");
                                            needs_redraw = true;
                                        }
                                    }
                                } else {
                                    needs_redraw = true;
                                }
                            }
                        }
                        crate::editor::NormalInputResult::None => {
                            needs_redraw = true;
                        }
                    }
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
