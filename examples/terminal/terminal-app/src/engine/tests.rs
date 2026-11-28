use super::*;

fn engine(cols: u16, rows: u16) -> Engine {
    Engine::new(Size { cols, rows }).unwrap()
}

fn key(name: &str, text: Option<&str>) -> Key {
    Key {
        key: name.into(),
        text: text.map(Into::into),
        ..Key::default()
    }
}

#[test]
fn text_is_split_into_spans_by_style() {
    let mut e = engine(20, 3);
    e.feed(b"\x1b[31mred\x1b[0m plain\r\n\x1b[1;4mbold\x1b[0m");
    let screen = e.screen().unwrap();

    assert_eq!(screen.text(), ["red plain", "bold", ""]);
    let first = &screen.lines[0];
    assert_eq!(first[0].text, "red");
    assert!(first[0].fg.is_some());
    assert_eq!(first[1].text, " plain");
    assert_eq!(first[1].fg, None);
    assert!(screen.lines[1][0].bold && screen.lines[1][0].underline);
    assert_eq!(screen.cursor, Some((1, 4)));
}

#[test]
fn trailing_blanks_are_dropped_unless_they_have_a_background() {
    let mut e = engine(10, 2);
    e.feed(b"ab   \r\n\x1b[44mbar  \x1b[0m");
    let screen = e.screen().unwrap();

    assert_eq!(screen.lines[0].len(), 1);
    assert_eq!(screen.lines[0][0].text, "ab");
    assert_eq!(screen.lines[1][0].text, "bar  ");
    assert!(screen.lines[1][0].bg.is_some());
}

#[test]
fn inverse_swaps_the_colors() {
    let mut e = engine(10, 1);
    e.feed(b"\x1b[7mx");
    let span = &e.screen().unwrap().lines[0][0];

    assert!(span.fg.is_some() && span.bg.is_some());
    assert_ne!(span.fg, span.bg);
}

#[test]
fn a_wide_character_takes_one_column_and_the_cursor_follows() {
    let mut e = engine(10, 1);
    e.feed("日本x".as_bytes());
    let screen = e.screen().unwrap();

    assert_eq!(screen.text(), ["日本x"]);
    assert_eq!(screen.cursor, Some((0, 3)));
}

#[test]
fn a_hidden_cursor_is_not_shown() {
    let mut e = engine(10, 1);
    e.feed(b"\x1b[?25l");
    assert_eq!(e.screen().unwrap().cursor, None);
}

#[test]
fn queries_titles_directories_and_bells_are_reported() {
    let mut e = engine(10, 2);
    let reports = e.feed(b"\x1b[2;3H\x1b[6n\x1b]2;vim\x07\x1b]7;file://host/tmp\x07\x07");

    assert_eq!(
        reports,
        [
            Report::Reply(b"\x1b[2;3R".to_vec()),
            Report::Title("vim".into()),
            Report::Cwd("file://host/tmp".into()),
            Report::Bell,
        ]
    );
}

#[test]
fn resizing_changes_the_grid() {
    let mut e = engine(10, 2);
    e.resize(Size { cols: 4, rows: 5 }).unwrap();

    assert_eq!(e.screen().unwrap().lines.len(), 5);
    let reports = e.feed(b"\x1b[18t");
    assert_eq!(reports, [Report::Reply(b"\x1b[8;5;4t".to_vec())]);
}

#[test]
fn keys_follow_the_modes_the_program_set() {
    let mut e = engine(10, 1);
    assert_eq!(e.key(&key("a", Some("a"))).unwrap(), b"a");
    assert_eq!(e.key(&key("enter", Some("\n"))).unwrap(), b"\r");
    assert_eq!(e.key(&key("up", None)).unwrap(), b"\x1b[A");

    let ctrl_c = Key {
        ctrl: true,
        ..key("c", None)
    };
    assert_eq!(e.key(&ctrl_c).unwrap(), [0x03]);
    let shift_a = Key {
        shift: true,
        ..key("a", Some("A"))
    };
    assert_eq!(e.key(&shift_a).unwrap(), b"A");

    // Application cursor keys, as vim and less ask for.
    e.feed(b"\x1b[?1h");
    assert_eq!(e.key(&key("up", None)).unwrap(), b"\x1bOA");
}

#[test]
fn keys_libghostty_does_not_name_type_their_text() {
    let mut e = engine(10, 1);
    assert_eq!(e.key(&key("é", Some("é"))).unwrap(), "é".as_bytes());
    let ctrl = Key {
        ctrl: true,
        ..key("é", Some("é"))
    };
    assert_eq!(e.key(&ctrl).unwrap(), b"");
    assert_eq!(e.key(&key("shift", None)).unwrap(), b"");
}

#[test]
fn pastes_are_bracketed_when_the_program_asks() {
    let mut e = engine(10, 1);
    assert_eq!(e.paste("ls\n").unwrap(), b"ls\r");

    e.feed(b"\x1b[?2004h");
    assert_eq!(e.paste("ls\n").unwrap(), b"\x1b[200~ls\n\x1b[201~");
}

#[test]
fn every_named_key_has_a_code() {
    let names = [
        "enter",
        "tab",
        "backspace",
        "escape",
        "space",
        "up",
        "down",
        "left",
        "right",
        "home",
        "end",
        "pageup",
        "pagedown",
        "insert",
        "delete",
        "f1",
        "f12",
        "a",
        "z",
        "0",
        "9",
        "`",
        "\\",
        "[",
        "]",
        ",",
        "=",
        "-",
        ".",
        "'",
        ";",
        "/",
        " ",
    ];
    for name in names {
        assert!(code(name).is_some(), "{name}");
    }
    for name in ["f0", "f13", "shift", "é", ""] {
        assert!(code(name).is_none(), "{name}");
    }
}
