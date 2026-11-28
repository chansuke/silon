use std::time::Duration;

use futures::StreamExt;
use silon_core::Mailboxes;
use terminal_core::{Key, Size};

use super::*;

fn sh(id: u32, script: &str) -> PtyKey {
    PtyKey {
        id,
        shell: vec!["/bin/sh".into(), "-c".into(), script.into()],
    }
}

/// Collects actions until `done` says so, failing after five seconds.
fn collect_until(
    stream: &mut BoxStream<'static, TermAction>,
    done: impl Fn(&TermAction) -> bool,
) -> Vec<TermAction> {
    smol::block_on(async {
        let mut seen = Vec::new();
        let deadline = smol::Timer::after(Duration::from_secs(5));
        let collect = async {
            while let Some(action) = stream.next().await {
                let stop = done(&action);
                seen.push(action);
                if stop {
                    return true;
                }
            }
            false
        };
        let finished = smol::future::or(collect, async {
            deadline.await;
            false
        })
        .await;
        assert!(finished, "gave up waiting; saw {seen:?}");
        seen
    })
}

fn shows(action: &TermAction, text: &str) -> bool {
    matches!(action, TermAction::Painted { screen, .. }
        if screen.text().iter().any(|line| line.contains(text)))
}

#[test]
fn the_shell_starts_at_the_posted_size_and_its_screen_comes_back() {
    let key = sh(3, "stty size; echo ready; sleep 5");
    let boxes = Mailboxes::new();
    let mut stream = start(key.clone(), boxes.open(key.clone()), None);

    boxes
        .post(&key, Input::Resize(Size { cols: 33, rows: 7 }))
        .unwrap();
    let seen = collect_until(&mut stream, |a| shows(a, "ready"));

    let TermAction::Painted { id, screen } = seen.last().unwrap() else {
        unreachable!()
    };
    assert_eq!(*id, 3);
    assert_eq!(screen.text()[0], "7 33");
    assert_eq!(screen.lines.len(), 7);
}

#[test]
fn keys_and_pastes_typed_before_the_size_are_kept() {
    let key = sh(1, "read a; read b; echo got:$a:$b; sleep 5");
    let boxes = Mailboxes::new();
    let mut stream = start(key.clone(), boxes.open(key.clone()), None);

    boxes.post(&key, Input::Paste("one\n".into())).unwrap();
    boxes.post(&key, Input::Resize(Size::default())).unwrap();
    boxes.post(&key, Input::Paste("two".into())).unwrap();
    let enter = Key {
        key: "enter".into(),
        ..Key::default()
    };
    boxes.post(&key, Input::Key(enter)).unwrap();

    collect_until(&mut stream, |a| shows(a, "got:one:two"));
}

#[test]
fn the_program_ending_ends_the_stream_with_its_exit_code() {
    let key = sh(2, "printf '\\033]2;job\\007'; exit 4");
    let boxes = Mailboxes::new();
    let mut stream = start(key.clone(), boxes.open(key.clone()), None);
    boxes.post(&key, Input::Resize(Size::default())).unwrap();

    let seen = collect_until(&mut stream, |a| matches!(a, TermAction::Exited { .. }));

    assert!(seen.contains(&TermAction::Titled {
        id: 2,
        title: "job".into()
    }));
    assert_eq!(
        seen.last(),
        Some(&TermAction::Exited {
            id: 2,
            code: Some(4)
        })
    );
    assert!(smol::block_on(stream.next()).is_none());
}

#[test]
fn dropping_the_stream_hangs_up_the_program() {
    let dir = std::env::temp_dir().join(format!("silon-session-{}", std::process::id()));
    let _ = std::fs::remove_file(&dir);
    let script = format!(
        "trap 'echo hup > {}; exit' HUP; echo ready; while :; do sleep 0.1; done",
        dir.display()
    );
    let key = sh(5, &script);
    let boxes = Mailboxes::new();
    let mut stream = start(key.clone(), boxes.open(key.clone()), None);
    boxes.post(&key, Input::Resize(Size::default())).unwrap();
    collect_until(&mut stream, |a| shows(a, "ready"));

    drop(stream);

    let hung_up = (0..200).any(|_| {
        std::thread::sleep(Duration::from_millis(10));
        std::fs::read_to_string(&dir).is_ok_and(|s| s.contains("hup"))
    });
    let _ = std::fs::remove_file(&dir);
    assert!(hung_up, "the program did not get SIGHUP");
    assert!(boxes.post(&key, Input::Paste("x".into())).is_err());
}

#[test]
fn osc_7_uris_become_paths() {
    assert_eq!(path_of("file://host/home/me"), "/home/me");
    assert_eq!(path_of("file:///tmp"), "/tmp");
    assert_eq!(path_of("/plain"), "/plain");
}
