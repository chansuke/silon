//! One session's VT emulator: libghostty-vt behind a sans-IO interface.
//! Bytes from the program go in, and screens, replies and reports come out.
//! It never touches the PTY, so tests feed it bytes. libghostty-vt handles
//! are `!Send`, so an engine lives on the thread that created it.

use ghostty_vt::key::{self, Action, Encoder, Mods};
use ghostty_vt::render::{CellIterator, RenderState, RowIterator};
use ghostty_vt::screen::{CellWide, Screen as ScreenKind};
use ghostty_vt::style::{RgbColor, Underline};
use ghostty_vt::terminal::{Event, Mode, Options, ScrollViewport, Terminal};
use terminal_core::{Key, Rgb, Screen, Size, Span};

pub use ghostty_vt::Error;

/// What the program asked for besides drawing.
#[derive(Debug, PartialEq, Eq)]
pub enum Report {
    /// Bytes for the program, such as the answer to a cursor query.
    Reply(Vec<u8>),
    Title(String),
    /// The working directory, as the shell reported it (OSC 7).
    Cwd(String),
    Bell,
}

pub struct Engine {
    terminal: Terminal,
    render: RenderState,
    rows: RowIterator,
    cells: CellIterator,
    keys: Encoder,
    event: key::Event,
}

fn content_color(color: RgbColor) -> Rgb {
    Rgb(color.r, color.g, color.b)
}

impl Engine {
    pub fn new(size: Size) -> Result<Self, Error> {
        Ok(Self {
            terminal: Terminal::new(Options {
                cols: size.cols.max(1),
                rows: size.rows.max(1),
                ..Options::default()
            })?,
            render: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
            keys: Encoder::new()?,
            event: key::Event::new()?,
        })
    }

    /// Feeds the program's output and returns what it asked for.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Report> {
        self.terminal.vt_write(bytes);
        self.terminal
            .events()
            .filter_map(|event| match event {
                Event::PtyWrite(bytes) => Some(Report::Reply(bytes)),
                Event::TitleChanged(title) => Some(Report::Title(title)),
                Event::PwdChanged(cwd) => Some(Report::Cwd(cwd)),
                Event::Bell => Some(Report::Bell),
                _ => None,
            })
            .collect()
    }

    pub fn resize(&mut self, size: Size) -> Result<(), Error> {
        self.terminal
            .resize(size.cols.max(1), size.rows.max(1), 0, 0)
    }

    /// Scrolls `lines`, up when negative. Full-screen programs (the
    /// alternate screen) get arrow keys instead, as with xterm's alternate
    /// scroll mode; returns the bytes to send them.
    pub fn scroll(&mut self, lines: i32) -> Result<Vec<u8>, Error> {
        if self.terminal.active_screen()? == ScreenKind::Alternate {
            let name = if lines < 0 { "up" } else { "down" };
            let arrow = Key {
                key: name.into(),
                ..Key::default()
            };
            let once = self.key(&arrow)?;
            return Ok(once.repeat(lines.unsigned_abs() as usize));
        }
        self.terminal
            .scroll_viewport(ScrollViewport::Delta(lines as isize));
        Ok(Vec::new())
    }

    /// The bytes for `key`, in the encoding the program asked for. Typing
    /// brings the view back to the latest output.
    pub fn key(&mut self, key: &Key) -> Result<Vec<u8>, Error> {
        self.terminal.scroll_viewport(ScrollViewport::Bottom);
        let Some(code) = code(&key.key) else {
            // Keys libghostty does not name still type their text.
            return Ok(match (&key.text, key.ctrl || key.alt) {
                (Some(text), false) => text.as_bytes().to_vec(),
                _ => Vec::new(),
            });
        };
        let mut mods = Mods::empty();
        mods.set(Mods::CTRL, key.ctrl);
        mods.set(Mods::ALT, key.alt);
        mods.set(Mods::SHIFT, key.shift);
        let text = key
            .text
            .clone()
            .filter(|t| !t.chars().any(char::is_control));
        self.event
            .set_action(Action::Press)
            .set_key(code)
            .set_mods(mods)
            .set_consumed_mods(if text.is_some() && key.shift {
                Mods::SHIFT
            } else {
                Mods::empty()
            })
            .set_utf8(text);
        if let Some(c) = key
            .key
            .chars()
            .next()
            .filter(|_| key.key.chars().count() == 1)
        {
            self.event.set_unshifted_codepoint(c);
        }
        self.keys.set_options_from_terminal(&self.terminal);
        let mut bytes = Vec::new();
        self.keys.encode_to_vec(&self.event, &mut bytes)?;
        Ok(bytes)
    }

    /// The bytes for typed `text`: the text itself. Typing brings the view
    /// back to the latest output.
    pub fn text(&mut self, text: &str) -> Vec<u8> {
        self.terminal.scroll_viewport(ScrollViewport::Bottom);
        text.as_bytes().to_vec()
    }

    /// The bytes for pasting `text`: bracketed when the program asked for
    /// it, with control bytes made harmless.
    pub fn paste(&mut self, text: &str) -> Result<Vec<u8>, Error> {
        self.terminal.scroll_viewport(ScrollViewport::Bottom);
        let bracketed = self.terminal.mode(Mode::BRACKETED_PASTE)?;
        let mut data = text.as_bytes().to_vec();
        let mut buf = vec![0; data.len() + 16];
        loop {
            match ghostty_vt::paste::encode(&mut data, bracketed, &mut buf) {
                Ok(written) => {
                    buf.truncate(written);
                    return Ok(buf);
                }
                Err(Error::OutOfSpace { required }) => buf.resize(required, 0),
                Err(err) => return Err(err),
            }
        }
    }

    /// The visible screen, as styled spans.
    pub fn screen(&mut self) -> Result<Screen, Error> {
        let snapshot = self.render.update(&self.terminal)?;
        let colors = snapshot.colors()?;
        let cursor = snapshot.cursor()?;
        let cursor_at = cursor
            .viewport
            .filter(|_| cursor.visible)
            .map(|c| (c.y, c.x));
        let mut lines = Vec::new();
        let mut cursor_col = None;
        let mut rows = self.rows.update(&snapshot)?;
        let mut y = 0u16;
        while let Some(row) = rows.next() {
            let mut cells = self.cells.update(row)?;
            let mut spans: Vec<Span> = Vec::new();
            // Characters written so far, and where the line's styled or
            // non-blank content ends.
            let mut chars = 0u16;
            let mut x = 0u16;
            let mut content_end = (0usize, 0usize);
            while let Some(cell) = cells.next() {
                if cursor_at == Some((y, x)) {
                    cursor_col = Some((y, chars));
                }
                x += 1;
                if matches!(
                    cell.raw_cell()?.wide()?,
                    CellWide::SpacerTail | CellWide::SpacerHead
                ) {
                    continue;
                }
                let style = cell.style()?;
                let (mut fg, mut bg) = (
                    cell.fg_color()?.map(content_color),
                    cell.bg_color()?.map(content_color),
                );
                if style.inverse {
                    let fg_or_default = fg.unwrap_or(content_color(colors.foreground));
                    fg = Some(bg.unwrap_or(content_color(colors.background)));
                    bg = Some(fg_or_default);
                }
                let graphemes = cell.graphemes()?;
                let blank = graphemes.is_empty() || graphemes == [' '];
                let text: String = if graphemes.is_empty() {
                    " ".into()
                } else {
                    graphemes.into_iter().collect()
                };
                let span = Span {
                    text,
                    fg,
                    bg,
                    bold: style.bold,
                    italic: style.italic,
                    underline: style.underline != Underline::None,
                };
                let plain = blank && bg.is_none() && !span.underline;
                match spans.last_mut() {
                    Some(last)
                        if (last.fg, last.bg, last.bold, last.italic, last.underline)
                            == (span.fg, span.bg, span.bold, span.italic, span.underline) =>
                    {
                        last.text.push_str(&span.text);
                    }
                    _ => spans.push(span),
                }
                chars += 1;
                if !plain {
                    let last = spans.len() - 1;
                    content_end = (spans.len(), spans[last].text.chars().count());
                }
            }
            if cursor_at == Some((y, x)) {
                cursor_col = Some((y, chars));
            }
            // Drop the blank, unstyled cells after the last content.
            let (keep, chars_in_last) = content_end;
            spans.truncate(keep);
            if let Some(last) = spans.last_mut() {
                last.text = last.text.chars().take(chars_in_last).collect();
            }
            lines.push(spans);
            y += 1;
        }
        Ok(Screen {
            lines,
            cursor: cursor_col,
            scrolled_back: !self.terminal.is_viewport_active()?,
        })
    }
}

/// libghostty's code for a key named the way GPUI names it.
fn code(name: &str) -> Option<key::Key> {
    use key::Key as K;
    let letters = [
        K::A,
        K::B,
        K::C,
        K::D,
        K::E,
        K::F,
        K::G,
        K::H,
        K::I,
        K::J,
        K::K,
        K::L,
        K::M,
        K::N,
        K::O,
        K::P,
        K::Q,
        K::R,
        K::S,
        K::T,
        K::U,
        K::V,
        K::W,
        K::X,
        K::Y,
        K::Z,
    ];
    let digits = [
        K::Digit0,
        K::Digit1,
        K::Digit2,
        K::Digit3,
        K::Digit4,
        K::Digit5,
        K::Digit6,
        K::Digit7,
        K::Digit8,
        K::Digit9,
    ];
    let functions = [
        K::F1,
        K::F2,
        K::F3,
        K::F4,
        K::F5,
        K::F6,
        K::F7,
        K::F8,
        K::F9,
        K::F10,
        K::F11,
        K::F12,
    ];
    if let [c] = name.as_bytes() {
        let c = c.to_ascii_lowercase();
        return match c {
            b'a'..=b'z' => Some(letters[usize::from(c - b'a')]),
            b'0'..=b'9' => Some(digits[usize::from(c - b'0')]),
            b'`' => Some(K::Backquote),
            b'\\' => Some(K::Backslash),
            b'[' => Some(K::BracketLeft),
            b']' => Some(K::BracketRight),
            b',' => Some(K::Comma),
            b'=' => Some(K::Equal),
            b'-' => Some(K::Minus),
            b'.' => Some(K::Period),
            b'\'' => Some(K::Quote),
            b';' => Some(K::Semicolon),
            b'/' => Some(K::Slash),
            b' ' => Some(K::Space),
            _ => None,
        };
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<usize>().ok()) {
        return n.checked_sub(1).and_then(|i| functions.get(i).copied());
    }
    Some(match name {
        "enter" => K::Enter,
        "tab" => K::Tab,
        "backspace" => K::Backspace,
        "escape" => K::Escape,
        "space" => K::Space,
        "up" => K::ArrowUp,
        "down" => K::ArrowDown,
        "left" => K::ArrowLeft,
        "right" => K::ArrowRight,
        "home" => K::Home,
        "end" => K::End,
        "pageup" => K::PageUp,
        "pagedown" => K::PageDown,
        "insert" => K::Insert,
        "delete" => K::Delete,
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
