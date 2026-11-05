//! Key bindings.

use std::collections::{BTreeMap, HashMap};

use anyhow::{Result, bail};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Everything a key can do.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    ScrollDown,
    ScrollUp,
    ScrollLeft,
    ScrollRight,
    ScreenDown,
    ScreenUp,
    HalfDown,
    HalfUp,
    NextPage,
    PrevPage,
    FirstPage,
    /// Last page, or page n with a count.
    LastPage,
    ZoomIn,
    ZoomOut,
    ZoomText,
    FitWidth,
    FitPage,
    Lens,
    Search,
    SearchNext,
    SearchPrev,
    Outline,
    NextLink,
    PrevLink,
    FollowLink,
    Copy,
    Info,
    Help,
    Reflow,
    Raster,
    Reload,
    Quit,
}

impl Action {
    pub const ALL: [(Action, &'static str, &'static str); 32] = [
        (Action::ScrollDown, "scroll_down", "scroll down"),
        (Action::ScrollUp, "scroll_up", "scroll up"),
        (Action::ScrollLeft, "scroll_left", "scroll left"),
        (Action::ScrollRight, "scroll_right", "scroll right"),
        (Action::ScreenDown, "screen_down", "one screen down"),
        (Action::ScreenUp, "screen_up", "one screen up"),
        (Action::HalfDown, "half_down", "half a screen down"),
        (Action::HalfUp, "half_up", "half a screen up"),
        (Action::NextPage, "next_page", "next page"),
        (Action::PrevPage, "prev_page", "previous page"),
        (Action::FirstPage, "first_page", "first page"),
        (Action::LastPage, "last_page", "last page (or page n: type n first)"),
        (Action::ZoomIn, "zoom_in", "zoom in (lens: magnify)"),
        (Action::ZoomOut, "zoom_out", "zoom out (lens: magnify less)"),
        (Action::ZoomText, "zoom_text", "text zoom: one row per line of text"),
        (Action::FitWidth, "fit_width", "fit page width"),
        (Action::FitPage, "fit_page", "fit whole page"),
        (Action::Lens, "lens", "lens on/off (arrows or hjkl move it)"),
        (Action::Search, "search", "search"),
        (Action::SearchNext, "search_next", "next match"),
        (Action::SearchPrev, "search_prev", "previous match"),
        (Action::Outline, "outline", "outline (table of contents)"),
        (Action::NextLink, "next_link", "next link on screen"),
        (Action::PrevLink, "prev_link", "previous link on screen"),
        (Action::FollowLink, "follow_link", "follow link / open figure"),
        (Action::Copy, "copy", "copy page text (OSC 52)"),
        (Action::Info, "info", "info: page, zoom, tier, colors, frame bytes"),
        (Action::Help, "help", "this help"),
        (Action::Reflow, "reflow", "reflow mode on/off"),
        (Action::Raster, "raster", "raster mode on/off"),
        (Action::Reload, "reload", "reload the file"),
        (Action::Quit, "quit", "quit"),
    ];

    pub fn name(self) -> &'static str {
        Action::ALL
            .iter()
            .find(|(a, _, _)| *a == self)
            .map_or("?", |(_, n, _)| n)
    }

    pub fn describe(self) -> &'static str {
        Action::ALL
            .iter()
            .find(|(a, _, _)| *a == self)
            .map_or("?", |(_, _, d)| d)
    }

    fn by_name(name: &str) -> Option<Action> {
        Action::ALL.iter().find(|(_, n, _)| *n == name).map(|(a, _, _)| *a)
    }
}

/// A key: code plus the modifiers that matter (ctrl, alt).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub ctrl: bool,
    pub alt: bool,
}

impl Key {
    pub fn from_event(e: &KeyEvent) -> Key {
        // Shift is part of the character for printable keys ('J' vs 'j').
        let code = match e.code {
            KeyCode::Char(c) if e.modifiers.contains(KeyModifiers::SHIFT) => KeyCode::Char(c.to_ascii_uppercase()),
            c => c,
        };
        Key {
            code,
            ctrl: e.modifiers.contains(KeyModifiers::CONTROL),
            alt: e.modifiers.contains(KeyModifiers::ALT),
        }
    }

    fn plain(code: KeyCode) -> Key {
        Key {
            code,
            ctrl: false,
            alt: false,
        }
    }

    /// Parse "j", "J", "space", "pgdn", "ctrl-d", "alt-x", "enter", "f5"...
    pub fn parse(s: &str) -> Result<Key> {
        let mut ctrl = false;
        let mut alt = false;
        let mut rest = s;
        loop {
            if let Some(r) = rest.strip_prefix("ctrl-").or_else(|| rest.strip_prefix("C-")) {
                ctrl = true;
                rest = r;
            } else if let Some(r) = rest.strip_prefix("alt-").or_else(|| rest.strip_prefix("M-")) {
                alt = true;
                rest = r;
            } else {
                break;
            }
        }
        let code = match rest.to_ascii_lowercase().as_str() {
            "space" => KeyCode::Char(' '),
            "enter" | "return" => KeyCode::Enter,
            "tab" => KeyCode::Tab,
            "backtab" | "shift-tab" => KeyCode::BackTab,
            "esc" | "escape" => KeyCode::Esc,
            "backspace" => KeyCode::Backspace,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "pgup" | "pageup" => KeyCode::PageUp,
            "pgdn" | "pagedown" => KeyCode::PageDown,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            f if f.starts_with('f') && f.len() > 1 && f[1..].parse::<u8>().is_ok() => {
                KeyCode::F(f[1..].parse().expect("checked"))
            }
            _ => {
                let mut chars = rest.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => KeyCode::Char(if ctrl { c.to_ascii_lowercase() } else { c }),
                    _ => bail!("unknown key {s:?}"),
                }
            }
        };
        Ok(Key { code, ctrl, alt })
    }

    pub fn display(&self) -> String {
        let base = match self.code {
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Enter => "Enter".into(),
            KeyCode::Tab => "Tab".into(),
            KeyCode::BackTab => "S-Tab".into(),
            KeyCode::Esc => "Esc".into(),
            KeyCode::Up => "↑".into(),
            KeyCode::Down => "↓".into(),
            KeyCode::Left => "←".into(),
            KeyCode::Right => "→".into(),
            KeyCode::PageUp => "PgUp".into(),
            KeyCode::PageDown => "PgDn".into(),
            KeyCode::Home => "Home".into(),
            KeyCode::End => "End".into(),
            KeyCode::F(n) => format!("F{n}"),
            other => format!("{other:?}"),
        };
        match (self.ctrl, self.alt) {
            (true, _) => format!("C-{base}"),
            (_, true) => format!("M-{base}"),
            _ => base,
        }
    }
}

/// Keys → actions.
pub struct Keymap {
    map: HashMap<Key, Action>,
}

impl Keymap {
    /// The default bindings plus the ones from the config file.
    pub fn new(extra: &BTreeMap<String, Vec<String>>) -> Result<Keymap> {
        use Action::*;
        use KeyCode::*;
        let mut map = HashMap::new();
        let defaults: &[(KeyCode, Action)] = &[
            (Char('j'), ScrollDown),
            (Down, ScrollDown),
            (Char('k'), ScrollUp),
            (Up, ScrollUp),
            (Char('h'), ScrollLeft),
            (Left, ScrollLeft),
            (Char('l'), ScrollRight),
            (Right, ScrollRight),
            (Char(' '), ScreenDown),
            (PageDown, ScreenDown),
            (Char('b'), ScreenUp),
            (PageUp, ScreenUp),
            (Char('J'), NextPage),
            (Char('K'), PrevPage),
            (Char('g'), FirstPage),
            (Home, FirstPage),
            (Char('G'), LastPage),
            (End, LastPage),
            (Char('+'), ZoomIn),
            (Char('-'), ZoomOut),
            (Char('='), ZoomText),
            (Char('w'), FitWidth),
            (Char('p'), FitPage),
            (Char('z'), Lens),
            (Char('/'), Search),
            (Char('n'), SearchNext),
            (Char('N'), SearchPrev),
            (Char('o'), Outline),
            (Tab, NextLink),
            (BackTab, PrevLink),
            (Enter, FollowLink),
            (Char('y'), Copy),
            (Char('i'), Info),
            (Char('?'), Help),
            (Char('r'), Reflow),
            (Char('R'), Raster),
            (Char('q'), Quit),
        ];
        for (code, action) in defaults {
            map.insert(Key::plain(*code), *action);
        }
        for (code, action) in [
            ('d', HalfDown),
            ('u', HalfUp),
            ('f', ScreenDown),
            ('b', ScreenUp),
            ('l', Reload),
            ('c', Quit),
        ] {
            map.insert(
                Key {
                    code: Char(code),
                    ctrl: true,
                    alt: false,
                },
                action,
            );
        }
        for (name, keys) in extra {
            let Some(action) = Action::by_name(name) else {
                bail!("unknown action {name:?} in [keys]");
            };
            for k in keys {
                map.insert(Key::parse(k)?, action);
            }
        }
        Ok(Keymap { map })
    }

    pub fn action(&self, key: &Key) -> Option<Action> {
        self.map.get(key).copied()
    }

    /// Keys bound to an action, for the help screen.
    pub fn keys_for(&self, action: Action) -> Vec<String> {
        let mut keys: Vec<String> = self
            .map
            .iter()
            .filter(|(_, a)| **a == action)
            .map(|(k, _)| k.display())
            .collect();
        keys.sort_by_key(|k| (k.chars().count(), k.clone()));
        keys
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keys() {
        assert_eq!(Key::parse("j").unwrap(), Key::plain(KeyCode::Char('j')));
        assert_eq!(Key::parse("J").unwrap(), Key::plain(KeyCode::Char('J')));
        assert_eq!(Key::parse("space").unwrap(), Key::plain(KeyCode::Char(' ')));
        let k = Key::parse("ctrl-d").unwrap();
        assert!(k.ctrl && k.code == KeyCode::Char('d'));
        assert_eq!(Key::parse("f5").unwrap(), Key::plain(KeyCode::F(5)));
        assert!(Key::parse("nope").is_err());
    }

    #[test]
    fn defaults_and_extras() {
        let mut extra = BTreeMap::new();
        extra.insert("next_page".to_string(), vec!["ctrl-n".to_string()]);
        let km = Keymap::new(&extra).unwrap();
        assert_eq!(km.action(&Key::plain(KeyCode::Char('J'))), Some(Action::NextPage));
        assert_eq!(km.action(&Key::parse("ctrl-n").unwrap()), Some(Action::NextPage));
        assert_eq!(km.action(&Key::plain(KeyCode::Char('q'))), Some(Action::Quit));
        let mut bad = BTreeMap::new();
        bad.insert("fly".to_string(), vec!["x".to_string()]);
        assert!(Keymap::new(&bad).is_err());
    }

    #[test]
    fn shifted_letters() {
        let e = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::SHIFT);
        assert_eq!(Key::from_event(&e).code, KeyCode::Char('J'));
    }
}
