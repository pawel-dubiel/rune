use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
    Visual,
    VisualLine,
    VisualBlock,
}

#[derive(Clone, Copy)]
pub enum Action {
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    LineStart,
    LineEnd,
    GotoTop,
    GotoBottom,
    EnterInsert,
    Append,
    OpenBelow,
    OpenAbove,
    DeleteCharUnder,
    DeleteLine,
    OperatorDelete,
    OperatorChange,
    OperatorYank,
    MoveWordForward,
    MoveWordBackward,
    MoveEndWord,
    Undo,
    Redo,
    CommandPrompt,
    EnterVisual,
    EnterVisualLine,
    EnterVisualBlock,
    PasteAfter,
    PasteBefore,
}

pub fn default_keymap() -> HashMap<String, Action> {
    use Action::*;
    let mut m = HashMap::new();
    m.insert("h".into(), MoveLeft);
    m.insert("j".into(), MoveDown);
    m.insert("k".into(), MoveUp);
    m.insert("l".into(), MoveRight);
    m.insert("0".into(), LineStart);
    m.insert("$".into(), LineEnd);
    m.insert("gg".into(), GotoTop);
    m.insert("G".into(), GotoBottom);
    m.insert("i".into(), EnterInsert);
    m.insert("v".into(), EnterVisual);
    m.insert("V".into(), EnterVisualLine);
    m.insert("a".into(), Append);
    m.insert("o".into(), OpenBelow);
    m.insert("O".into(), OpenAbove);
    m.insert("x".into(), DeleteCharUnder);
    m.insert("dd".into(), DeleteLine);
    m.insert("d".into(), OperatorDelete);
    m.insert("c".into(), OperatorChange);
    m.insert("y".into(), OperatorYank);
    m.insert("u".into(), Undo);
    m.insert("w".into(), MoveWordForward);
    m.insert("b".into(), MoveWordBackward);
    m.insert("e".into(), MoveEndWord);
    m.insert(":".into(), CommandPrompt);
    m.insert("p".into(), PasteAfter);
    m.insert("P".into(), PasteBefore);
    m
}

fn parse_action(name: &str) -> Option<Action> {
    use Action::*;
    match name.trim() {
        "move_left" | "h" => Some(MoveLeft),
        "move_down" | "j" => Some(MoveDown),
        "move_up" | "k" => Some(MoveUp),
        "move_right" | "l" => Some(MoveRight),
        "line_start" | "0" => Some(LineStart),
        "line_end" | "$" => Some(LineEnd),
        "goto_top" | "gg" => Some(GotoTop),
        "goto_bottom" | "G" => Some(GotoBottom),
        "insert" | "i" => Some(EnterInsert),
        "append" | "a" => Some(Append),
        "open_below" | "o" => Some(OpenBelow),
        "open_above" | "O" => Some(OpenAbove),
        "delete_char" | "x" => Some(DeleteCharUnder),
        "delete_line" | "dd" => Some(DeleteLine),
        "delete" | "d" => Some(OperatorDelete),
        "change" | "c" => Some(OperatorChange),
        "yank" | "y" => Some(OperatorYank),
        "undo" | "u" => Some(Undo),
        "redo" => Some(Redo),
        "move_word_forward" | "w" => Some(MoveWordForward),
        "move_word_backward" | "b" => Some(MoveWordBackward),
        "move_end_word" | "e" => Some(MoveEndWord),
        "command" | ":" => Some(CommandPrompt),
        "visual" | "v" => Some(EnterVisual),
        "visual_line" | "V" => Some(EnterVisualLine),
        "paste_after" | "p" => Some(PasteAfter),
        "paste_before" | "P" => Some(PasteBefore),
        _ => None,
    }
}

pub struct EditorConfig {
    pub keymap: HashMap<String, Action>,
    pub start_in_insert: bool,
}

pub fn load_config(mut base: HashMap<String, Action>) -> EditorConfig {
    // Search order (new name first, then legacy):
    // 1) ./rune.conf
    // 2) $XDG_CONFIG_HOME/rune/config.conf
    // 3) ~/.config/rune/config.conf
    // 4) ./vedit.conf
    // 5) $XDG_CONFIG_HOME/vedit/config.conf
    // 6) ~/.config/vedit/config.conf
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from("rune.conf"));
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let mut p = PathBuf::from(xdg);
        p.push("rune");
        p.push("config.conf");
        candidates.push(p);
    } else if let Ok(home) = std::env::var("HOME") {
        let mut p = PathBuf::from(home);
        p.push(".config/rune/config.conf");
        candidates.push(p);
    }
    // Legacy locations
    candidates.push(PathBuf::from("vedit.conf"));
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let mut p = PathBuf::from(xdg);
        p.push("vedit");
        p.push("config.conf");
        candidates.push(p);
    } else if let Ok(home) = std::env::var("HOME") {
        let mut p = PathBuf::from(home);
        p.push(".config/vedit/config.conf");
        candidates.push(p);
    }
    let mut start_in_insert = false;
    for path in candidates {
        if let Ok(content) = fs::read_to_string(&path) {
            let mut section = String::new();
            for line in content.lines() {
                let s = line.trim();
                if s.is_empty() || s.starts_with('#') {
                    continue;
                }
                if s.starts_with('[') && s.ends_with(']') {
                    section = s[1..s.len() - 1].to_string();
                    continue;
                }
                if let Some(eq) = s.find('=') {
                    let (lhs, rhs) = s.split_at(eq);
                    let key = lhs.trim();
                    let val = rhs[1..].trim(); // skip '='
                    match section.as_str() {
                        "normal" => {
                            let seq = key.trim().trim_matches('"');
                            if let Some(act) = parse_action(val) {
                                base.insert(seq.to_string(), act);
                            }
                        }
                        "general" => {
                            if key.eq_ignore_ascii_case("start_in_insert") {
                                let v = val.trim_matches('"').to_ascii_lowercase();
                                start_in_insert = matches!(v.as_str(), "1" | "true" | "yes" | "on");
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    EditorConfig {
        keymap: base,
        start_in_insert,
    }
}
