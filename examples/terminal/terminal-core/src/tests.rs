use silon_core::TestHarness;

use super::*;

fn shell() -> Vec<String> {
    vec!["/bin/sh".into()]
}

fn harness() -> TestHarness<Terminal> {
    TestHarness::new(TermState::new(shell()))
}

fn pty(id: u32) -> Pty {
    Pty { id, shell: shell() }
}

fn input(id: u32, input: Input) -> TermEffect {
    TermEffect::Input { to: pty(id), input }
}

fn key(name: &str) -> Key {
    Key {
        key: name.into(),
        text: Some(name.into()),
        ..Key::default()
    }
}

fn ids(h: &TestHarness<Terminal>) -> Vec<u32> {
    h.state.sessions.iter().map(|s| s.id).collect()
}

#[test]
fn a_new_session_runs_the_shell_at_the_current_size_and_becomes_active() {
    let mut h = harness();
    h.send(TermAction::NewSession).send(TermAction::NewSession);

    assert_eq!(ids(&h), [0, 1]);
    assert_eq!(h.state.active, Some(1));
    assert_eq!(h.state.active_session().map(|s| s.id), Some(1));
    assert_eq!(h.state.session(1).unwrap().title, "/bin/sh");
    assert_eq!(h.subscriptions(), [pty(0), pty(1)]);
    assert_eq!(
        h.take_effects(),
        [
            input(0, Input::Resize(Size::default())),
            input(1, Input::Resize(Size::default())),
        ]
    );
}

#[test]
fn closing_the_active_session_activates_its_neighbour() {
    let mut h = harness();
    for _ in 0..3 {
        h.send(TermAction::NewSession);
    }
    h.send(TermAction::Activate(1)).send(TermAction::Close(1));
    assert_eq!((ids(&h), h.state.active), (vec![0, 2], Some(2)));

    h.send(TermAction::Close(2));
    assert_eq!((ids(&h), h.state.active), (vec![0], Some(0)));
    assert_eq!(h.subscriptions(), [pty(0)]);

    h.send(TermAction::Close(0));
    assert_eq!(h.state.active, None);
    assert!(h.state.active_session().is_none());
    assert_eq!(h.subscriptions(), []);
}

#[test]
fn closing_another_session_keeps_the_active_one() {
    let mut h = harness();
    h.send(TermAction::NewSession)
        .send(TermAction::NewSession)
        .send(TermAction::Close(0))
        .send(TermAction::Close(9));

    assert_eq!((ids(&h), h.state.active), (vec![1], Some(1)));
}

#[test]
fn an_exited_program_closes_its_session() {
    let mut h = harness();
    h.send(TermAction::NewSession).send(TermAction::Exited {
        id: 0,
        code: Some(0),
    });

    assert!(h.state.sessions.is_empty());
    assert_eq!(h.subscriptions(), []);
}

#[test]
fn keys_and_pastes_go_to_the_active_session() {
    let mut h = harness();
    h.send(TermAction::NewSession)
        .send(TermAction::NewSession)
        .send(TermAction::Activate(0));
    h.take_effects();

    h.send(TermAction::Key(key("a")))
        .send(TermAction::Text("日本".into()))
        .send(TermAction::Text(String::new()))
        .send(TermAction::Paste("ls\n".into()))
        .send(TermAction::Paste(String::new()));

    assert_eq!(
        h.take_effects(),
        [
            input(0, Input::Key(key("a"))),
            input(0, Input::Text("日本".into())),
            input(0, Input::Paste("ls\n".into())),
        ]
    );
}

#[test]
fn keys_without_a_session_go_nowhere() {
    let mut h = harness();
    h.send(TermAction::Key(key("a")))
        .send(TermAction::Paste("x".into()));

    assert!(h.take_effects().is_empty());
}

#[test]
fn a_new_size_reaches_every_session_once() {
    let mut h = harness();
    h.send(TermAction::NewSession).send(TermAction::NewSession);
    h.take_effects();
    let size = Size { cols: 80, rows: 24 };

    h.send(TermAction::Resized(size))
        .send(TermAction::Resized(size))
        .send(TermAction::Resized(Size { cols: 0, rows: 24 }))
        .send(TermAction::Resized(Size { cols: 80, rows: 0 }));

    assert_eq!(h.state.size, size);
    assert_eq!(
        h.take_effects(),
        [input(0, Input::Resize(size)), input(1, Input::Resize(size))]
    );

    h.send(TermAction::NewSession);
    assert_eq!(h.take_effects(), [input(2, Input::Resize(size))]);
}

#[test]
fn session_reports_update_only_their_session() {
    let mut h = harness();
    h.send(TermAction::NewSession).send(TermAction::NewSession);
    let screen = Screen {
        lines: vec![vec![Span {
            text: "$ ls".into(),
            ..Span::default()
        }]],
        cursor: Some((0, 4)),
        scrolled_back: false,
    };

    h.send(TermAction::Painted {
        id: 0,
        screen: screen.clone(),
    })
    .send(TermAction::Titled {
        id: 0,
        title: "vim".into(),
    })
    .send(TermAction::MovedTo {
        id: 0,
        cwd: "file:///tmp".into(),
    })
    .send(TermAction::Painted {
        id: 7,
        screen: Screen::default(),
    });

    let first = h.state.session(0).unwrap();
    assert_eq!(first.screen.text(), ["$ ls"]);
    assert_eq!(first.title, "vim");
    assert_eq!(first.cwd.as_deref(), Some("file:///tmp"));
    assert_eq!(h.state.session(1).unwrap().screen, Screen::default());
}

#[test]
fn bells_count_until_the_session_is_activated() {
    let mut h = harness();
    h.send(TermAction::NewSession).send(TermAction::NewSession);

    h.send(TermAction::Rang { id: 0 })
        .send(TermAction::Rang { id: 0 })
        .send(TermAction::Rang { id: 1 });
    assert_eq!(h.state.session(0).unwrap().bells, 2);
    assert_eq!(h.state.session(1).unwrap().bells, 0, "the active one rang");

    h.send(TermAction::Activate(0));
    assert_eq!(h.state.session(0).unwrap().bells, 0);
}

#[test]
fn ids_are_never_reused() {
    let mut h = TestHarness::<Terminal>::new(TermState {
        next_id: u32::MAX,
        ..TermState::new(shell())
    });
    h.send(TermAction::NewSession);

    assert!(h.state.sessions.is_empty());
    assert!(h.take_effects().is_empty());
}

#[test]
fn shift_page_keys_scroll_history_instead_of_reaching_the_program() {
    let mut h = harness();
    h.send(TermAction::NewSession)
        .send(TermAction::Resized(Size { cols: 80, rows: 24 }));
    h.take_effects();
    let shifted = |name: &str| Key {
        shift: true,
        ..key(name)
    };

    h.send(TermAction::Key(shifted("pageup")))
        .send(TermAction::Key(shifted("pagedown")))
        .send(TermAction::Key(key("pageup")));

    assert_eq!(
        h.take_effects(),
        [
            input(0, Input::Scroll(-23)),
            input(0, Input::Scroll(23)),
            input(0, Input::Key(key("pageup"))),
        ]
    );
}

#[test]
fn a_one_row_terminal_still_pages_by_one_line() {
    let mut h = harness();
    h.send(TermAction::NewSession)
        .send(TermAction::Resized(Size { cols: 80, rows: 1 }));
    h.take_effects();
    h.send(TermAction::Key(Key {
        shift: true,
        ..key("pageup")
    }));

    assert_eq!(h.take_effects(), [input(0, Input::Scroll(-1))]);
}

#[test]
fn the_wheel_scrolls_the_active_session() {
    let mut h = harness();
    h.send(TermAction::NewSession);
    h.take_effects();

    h.send(TermAction::Scroll(-3)).send(TermAction::Scroll(0));

    assert_eq!(h.take_effects(), [input(0, Input::Scroll(-3))]);
}
