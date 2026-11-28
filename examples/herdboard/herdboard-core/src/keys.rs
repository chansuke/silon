//! Key presses, and the bytes a terminal program expects for them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One key press on the attached screen.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Key {
    /// The key's name: a character such as `a`, or `enter`, `up`, `f1`, ...
    pub key: String,
    /// The text the press types, with Shift and the layout applied.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
}

const ESC: u8 = 0x1b;

/// The bytes of named keys that are not text.
fn named(key: &str) -> Option<&'static [u8]> {
    Some(match key {
        "enter" => b"\r",
        "tab" => b"\t",
        "backspace" => b"\x7f",
        "escape" => b"\x1b",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "insert" => b"\x1b[2~",
        "delete" => b"\x1b[3~",
        "pageup" => b"\x1b[5~",
        "pagedown" => b"\x1b[6~",
        "f1" => b"\x1bOP",
        "f2" => b"\x1bOQ",
        "f3" => b"\x1bOR",
        "f4" => b"\x1bOS",
        _ => return None,
    })
}

/// Ctrl with a letter or one of `@[\]^_` and space gives a C0 control byte.
fn control(key: &str) -> Option<u8> {
    let [byte] = key.as_bytes() else {
        return None;
    };
    match byte.to_ascii_lowercase() {
        c @ b'a'..=b'z' => Some(c - b'a' + 1),
        b' ' | b'@' => Some(0),
        c @ (b'[' | b'\\' | b']' | b'^' | b'_') => Some(c - b'@'),
        _ => None,
    }
}

impl Key {
    /// The bytes to send for this press, or nothing for a press the
    /// terminal has no bytes for (a lone modifier, say).
    pub fn encode(&self) -> Vec<u8> {
        let key = if self.key == "space" { " " } else { &self.key };
        let base = match (named(key), self.ctrl.then(|| control(key)).flatten()) {
            (Some(bytes), _) => bytes.to_vec(),
            (None, Some(byte)) => vec![byte],
            (None, None) if self.ctrl => return Vec::new(),
            (None, None) => match &self.text {
                Some(text) => text.as_bytes().to_vec(),
                None => return Vec::new(),
            },
        };
        if self.alt {
            [&[ESC][..], &base].concat()
        } else {
            base
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: &str, text: Option<&str>) -> Key {
        Key {
            key: key.into(),
            text: text.map(Into::into),
            ..Key::default()
        }
    }

    fn ctrl(key: &str) -> Key {
        Key {
            ctrl: true,
            ..press(key, None)
        }
    }

    #[test]
    fn text_keys_send_the_text_they_type() {
        assert_eq!(press("a", Some("A")).encode(), b"A");
        assert_eq!(press("space", Some(" ")).encode(), b" ");
        assert_eq!(press("e", Some("é")).encode(), "é".as_bytes());
    }

    #[test]
    fn named_keys_send_their_sequences() {
        let expected: [(&str, &[u8]); 18] = [
            ("enter", b"\r"),
            ("tab", b"\t"),
            ("backspace", b"\x7f"),
            ("escape", b"\x1b"),
            ("up", b"\x1b[A"),
            ("down", b"\x1b[B"),
            ("right", b"\x1b[C"),
            ("left", b"\x1b[D"),
            ("home", b"\x1b[H"),
            ("end", b"\x1b[F"),
            ("insert", b"\x1b[2~"),
            ("delete", b"\x1b[3~"),
            ("pageup", b"\x1b[5~"),
            ("pagedown", b"\x1b[6~"),
            ("f1", b"\x1bOP"),
            ("f2", b"\x1bOQ"),
            ("f3", b"\x1bOR"),
            ("f4", b"\x1bOS"),
        ];
        for (key, bytes) in expected {
            // GPUI may give a named key text too; the name wins.
            assert_eq!(press(key, Some("?")).encode(), bytes, "{key}");
        }
    }

    #[test]
    fn ctrl_sends_control_bytes() {
        assert_eq!(ctrl("c").encode(), [0x03]);
        assert_eq!(ctrl("A").encode(), [0x01]);
        assert_eq!(ctrl("z").encode(), [0x1a]);
        assert_eq!(ctrl("space").encode(), [0x00]);
        assert_eq!(ctrl("[").encode(), [0x1b]);
        assert_eq!(ctrl("_").encode(), [0x1f]);
        assert_eq!(ctrl("enter").encode(), b"\r");
    }

    #[test]
    fn ctrl_with_a_key_without_a_control_byte_sends_nothing() {
        assert_eq!(ctrl("1").encode(), b"");
        let ctrl_one = Key {
            text: Some("1".into()),
            ..ctrl("1")
        };
        assert_eq!(ctrl_one.encode(), b"");
        assert_eq!(ctrl("ab").encode(), b"");
    }

    #[test]
    fn alt_prefixes_escape() {
        let alt_b = Key {
            alt: true,
            ..press("b", Some("b"))
        };
        assert_eq!(alt_b.encode(), b"\x1bb");
        let alt_ctrl_c = Key {
            alt: true,
            ..ctrl("c")
        };
        assert_eq!(alt_ctrl_c.encode(), [0x1b, 0x03]);
    }

    #[test]
    fn a_press_without_text_or_name_sends_nothing() {
        assert_eq!(press("shift", None).encode(), b"");
    }
}
