use silon_core::TestHarness;

use super::*;
use crate::wire::{decode_send_reply, decode_stream_line, send_request, subscribe_request};

/// A real stream from herdr-core 0.1.22, paths rewritten: the reply to
/// `events.subscribe` with `t1` running, then `t2` from start to removal.
const EVENTS: &str = include_str!("../tests/fixtures/events.jsonl");

fn lines() -> Vec<&'static str> {
    EVENTS.lines().collect()
}

fn decoded(index: usize) -> HerdAction {
    decode_stream_line(0, lines()[index]).expect("the dashboard follows this line")
}

fn terminal(address: &str, state: &str) -> Terminal {
    Terminal {
        address: address.into(),
        name: None,
        argv: vec!["sh".into()],
        cwd: "/work".into(),
        state: state.into(),
        status: None,
        title: String::new(),
        turn: 0,
        foreground: None,
        reason: None,
        exit_code: None,
    }
}

fn live(terminals: Vec<Terminal>) -> TestHarness<Herdboard> {
    let mut h = TestHarness::<Herdboard>::new(HerdState::new("/run/hc.sock"));
    h.send(HerdAction::Snapshot {
        attempt: 0,
        seq: 10,
        terminals,
    });
    h
}

fn changed(seq: u64, terminal: &str, change: Change) -> HerdAction {
    HerdAction::Changed {
        seq,
        terminal: terminal.into(),
        change,
    }
}

#[test]
fn the_recorded_stream_follows_a_terminal_from_start_to_removal() {
    let mut h = TestHarness::<Herdboard>::new(HerdState::new("/run/hc.sock"));
    let actions: Vec<_> = lines()
        .iter()
        .filter_map(|line| decode_stream_line(0, line))
        .collect();
    assert_eq!(actions.len(), lines().len());

    let mut seen = Vec::new();
    for action in actions {
        h.send(action);
        seen.push(h.state.terminal("t2").cloned());
    }

    let t1 = h.state.terminal("t1").unwrap();
    assert_eq!(
        (t1.name.as_deref(), t1.state.as_str()),
        (Some("demo"), "live")
    );
    assert_eq!(t1.foreground.as_ref().unwrap().name, "cat");

    let t2 = |i: usize| seen[i].clone().unwrap();
    assert_eq!(t2(1).name.as_deref(), Some("worker"));
    let working = Status {
        state: "working".into(),
        msg: "busy".into(),
    };
    assert_eq!(t2(2).status, Some(working));
    assert_eq!(t2(3).title, "job");
    assert_eq!(t2(4).foreground.unwrap().cwd, "/work/job");
    assert_eq!(t2(6).turn, 1);
    assert_eq!(t2(7).status, None);
    let finished = t2(8);
    assert_eq!(
        (
            finished.state.as_str(),
            finished.reason.as_deref(),
            finished.exit_code
        ),
        ("finished", Some("exited"), Some(3))
    );
    assert_eq!(finished.foreground, None);
    assert!(seen[9].is_none());

    assert_eq!(h.state.seq, 11);
    assert_eq!(h.state.link, Link::Live);
    assert_eq!(h.state.selected.as_deref(), Some("t1"));
    assert!(h.emitted_effects.is_empty());
}

#[test]
fn a_snapshot_from_an_older_attempt_changes_nothing() {
    let mut h = TestHarness::<Herdboard>::new(HerdState {
        attempt: 2,
        ..HerdState::new("/run/hc.sock")
    });
    let before = h.state.clone();

    h.send(HerdAction::Snapshot {
        attempt: 1,
        seq: 5,
        terminals: vec![terminal("t1", "live")],
    });

    assert_eq!(h.state, before);
}

#[test]
fn events_at_or_below_the_snapshot_seq_are_dropped() {
    let mut h = live(vec![terminal("t1", "live")]);

    let title = |title: &str| Change::Title {
        title: title.into(),
    };
    h.send(changed(10, "t1", title("old")))
        .send(changed(9, "t1", title("older")));
    assert_eq!(h.state.terminal("t1").unwrap().title, "");

    h.send(changed(11, "t1", title("new")));
    assert_eq!(h.state.terminal("t1").unwrap().title, "new");
    assert_eq!(h.state.seq, 11);
}

#[test]
fn events_before_the_first_snapshot_are_dropped() {
    let mut h = TestHarness::<Herdboard>::new(HerdState::new("/run/hc.sock"));
    h.send(changed(
        1,
        "t1",
        Change::Created(Box::new(terminal("t1", "live"))),
    ));

    assert!(h.state.terminals.is_empty());
    assert_eq!(h.state.seq, 0);
}

#[test]
fn a_missed_notification_replaces_the_list() {
    let mut h = live(vec![terminal("t1", "live"), terminal("t2", "live")]);
    let missed = r#"{"jsonrpc":"2.0","method":"missed","params":{"seq":57,"terminals":[{"address":"t3","state":"live"}]}}"#;

    h.send(decode_stream_line(0, missed).unwrap());

    let addresses: Vec<_> = h.state.terminals.iter().map(|t| &t.address).collect();
    assert_eq!(addresses, ["t3"]);
    assert_eq!(h.state.seq, 57);
    assert_eq!(h.state.selected.as_deref(), Some("t3"));
}

#[test]
fn the_end_of_a_stream_opens_the_next_attempt() {
    let mut h = live(vec![]);
    assert_eq!(
        h.subscriptions(),
        [Events {
            socket: "/run/hc.sock".into(),
            attempt: 0,
        }]
    );

    h.send(HerdAction::Disconnected {
        attempt: 0,
        error: "connection refused".into(),
    });

    assert_eq!(h.state.link, Link::Down("connection refused".into()));
    assert_eq!(
        h.subscriptions(),
        [Events {
            socket: "/run/hc.sock".into(),
            attempt: 1,
        }]
    );
}

#[test]
fn the_end_of_an_older_stream_changes_nothing() {
    let mut h = live(vec![]);
    h.send(HerdAction::Disconnected {
        attempt: 0,
        error: "eof".into(),
    })
    .send(HerdAction::Disconnected {
        attempt: 0,
        error: "eof again".into(),
    });

    assert_eq!(h.state.attempt, 1);
    assert_eq!(h.state.link, Link::Down("eof".into()));
}

#[test]
fn a_reply_error_ends_the_stream() {
    let line = r#"{"error":{"code":-32000,"message":"core is stopping"},"id":1,"jsonrpc":"2.0"}"#;

    assert_eq!(
        decode_stream_line(3, line),
        Some(HerdAction::Disconnected {
            attempt: 3,
            error: "core is stopping".into(),
        })
    );
}

#[test]
fn lines_the_dashboard_does_not_follow_decode_to_nothing() {
    let ignored = [
        r#"{"jsonrpc":"2.0","method":"heartbeat","params":{"seq":42}}"#,
        r#"{"jsonrpc":"2.0","method":"event","params":{"seq":43,"type":"doc","data":{"name":"app","version":2}}}"#,
        r#"{"jsonrpc":"2.0","method":"event","params":{"seq":44,"terminal":"t1","type":"size","data":{"cols":80,"rows":24}}}"#,
        "not json",
    ];
    for line in ignored {
        assert_eq!(decode_stream_line(0, line), None, "{line}");
    }
}

#[test]
fn removing_the_selected_terminal_selects_the_first_one_left() {
    let mut h = live(vec![terminal("t1", "live"), terminal("t2", "live")]);
    h.send(HerdAction::Select("t2".into()))
        .send(changed(11, "t2", Change::Removed {}));

    assert_eq!(h.state.selected.as_deref(), Some("t1"));
}

#[test]
fn selecting_an_unknown_terminal_keeps_the_selection() {
    let mut h = live(vec![terminal("t1", "live")]);
    h.send(HerdAction::Select("t9".into()));

    assert_eq!(h.state.selected.as_deref(), Some("t1"));
}

#[test]
fn send_hands_the_draft_to_the_selected_terminal() {
    let mut h = live(vec![terminal("t1", "live"), terminal("t2", "live")]);
    h.send(HerdAction::Select("t2".into()))
        .send(HerdAction::DraftChanged("run the tests".into()))
        .send(HerdAction::Send);

    assert_eq!(
        h.take_effects(),
        [HerdEffect::Send {
            socket: "/run/hc.sock".into(),
            terminal: "t2".into(),
            text: "run the tests".into(),
        }]
    );
    assert_eq!(h.state.draft, "");
    assert_eq!(
        h.state.notice,
        Some(Notice::Sending {
            terminal: "t2".into()
        })
    );
}

#[test]
fn send_does_nothing_without_a_live_target_or_text() {
    let cases = [
        ("blank draft", live(vec![terminal("t1", "live")]), "  "),
        ("finished", live(vec![terminal("t1", "finished")]), "hi"),
        ("no terminals", live(vec![]), "hi"),
        (
            "not connected",
            TestHarness::new(HerdState::new("/run/hc.sock")),
            "hi",
        ),
    ];
    for (case, mut h, draft) in cases {
        h.send(HerdAction::DraftChanged(draft.into()))
            .send(HerdAction::Send);

        assert!(h.emitted_effects.is_empty(), "{case}");
        assert_eq!(h.state.draft, draft, "{case}");
        assert_eq!(h.state.notice, None, "{case}");
    }
}

#[test]
fn the_send_reply_becomes_the_notice() {
    let ok = r#"{"id":1,"jsonrpc":"2.0","result":{"started":null,"status":null,"turn":4}}"#;
    let missing = r#"{"error":{"code":-32000,"data":{"hint":"see `hc ls --all`","kind":"not_found"},"message":"no terminal \"t9\""},"id":1,"jsonrpc":"2.0"}"#;
    let mut h = live(vec![terminal("t1", "live")]);

    h.send(decode_send_reply("t1", ok));
    assert_eq!(
        h.state.notice,
        Some(Notice::Sent {
            terminal: "t1".into(),
            turn: 4
        })
    );

    h.send(decode_send_reply("t9", missing));
    assert_eq!(
        h.state.notice,
        Some(Notice::Failed {
            terminal: "t9".into(),
            error: "no terminal \"t9\"".into()
        })
    );
}

#[test]
fn an_unreadable_send_reply_is_a_failure() {
    for line in ["", "{}", r#"{"result":{"turn":"x"}}"#] {
        let HerdAction::Sent { result, .. } = decode_send_reply("t1", line) else {
            panic!("decode_send_reply always answers Sent");
        };
        assert!(result.is_err(), "{line:?}");
    }
}

#[test]
fn requests_are_single_json_rpc_lines() {
    for request in [subscribe_request(), send_request("t1", "a\nb")] {
        let (body, rest) = request.split_once('\n').unwrap();
        assert_eq!(rest, "");
        let message: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(message["jsonrpc"], "2.0");
        assert_eq!(message["id"], 1);
    }
    let send: serde_json::Value =
        serde_json::from_str(send_request("t1", "a\nb").trim_end()).unwrap();
    assert_eq!(send["method"], "terminal.send");
    assert_eq!(send["params"]["text"], "a\nb");
}

#[test]
fn the_first_line_of_the_recording_is_the_subscribe_reply() {
    let HerdAction::Snapshot { seq, terminals, .. } = decoded(0) else {
        panic!("the reply is a snapshot");
    };
    assert_eq!(seq, 2);
    assert_eq!(terminals.len(), 1);
}
