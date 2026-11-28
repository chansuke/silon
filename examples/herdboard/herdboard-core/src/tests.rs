use std::time::Duration;

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

fn events(attempt: u32) -> Stream {
    Stream::Events {
        socket: "/run/hc.sock".into(),
        attempt,
    }
}

fn disconnected(attempt: u32, error: &str) -> HerdAction {
    HerdAction::Disconnected {
        attempt,
        error: error.into(),
    }
}

#[test]
fn the_end_of_a_stream_waits_then_opens_the_next_attempt() {
    let mut h = live(vec![]);
    assert_eq!(h.subscriptions(), [events(0)]);

    h.send(disconnected(0, "connection refused"));
    assert_eq!(
        h.state.link,
        Link::Waiting {
            error: "connection refused".into(),
            delay_ms: 500,
        }
    );
    assert_eq!(
        h.take_effects(),
        [HerdEffect::Wait {
            attempt: 0,
            delay: Duration::from_millis(500),
        }]
    );
    assert_eq!(h.subscriptions(), []);

    h.send(HerdAction::Retry { attempt: 0 });
    assert_eq!(h.state.link, Link::Connecting);
    assert_eq!(h.subscriptions(), [events(1)]);
}

#[test]
fn each_failure_in_a_row_doubles_the_wait_and_a_snapshot_resets_it() {
    let mut h = live(vec![]);
    let mut waits = Vec::new();
    for attempt in 0..3 {
        h.send(disconnected(attempt, "refused"))
            .send(HerdAction::Retry { attempt });
        waits.extend(h.take_effects());
    }
    let delays: Vec<_> = waits
        .iter()
        .map(|effect| match effect {
            HerdEffect::Wait { delay, .. } => delay.as_millis(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(delays, [500, 1000, 2000]);
    assert_eq!(h.state.failures, 3);

    h.send(HerdAction::Snapshot {
        attempt: 3,
        seq: 1,
        terminals: vec![],
    });
    assert_eq!(h.state.failures, 0);
    assert_eq!(h.state.link, Link::Live);
}

#[test]
fn the_wait_stops_growing_at_thirty_seconds() {
    assert_eq!(backoff(1), Duration::from_millis(500));
    assert_eq!(backoff(7), Duration::from_secs(30));
    assert_eq!(backoff(u32::MAX), Duration::from_secs(30));
}

#[test]
fn the_end_of_an_older_or_waiting_stream_changes_nothing() {
    let mut h = live(vec![]);
    h.send(disconnected(0, "eof"))
        .send(disconnected(0, "eof again"));

    assert_eq!(h.take_effects().len(), 1);
    assert_eq!(h.state.failures, 1);

    h.send(HerdAction::Retry { attempt: 0 })
        .send(disconnected(0, "late"))
        .send(HerdAction::Retry { attempt: 0 });
    assert_eq!(h.state.attempt, 1);
    assert_eq!(h.state.link, Link::Connecting);
    assert!(h.take_effects().is_empty());
}

#[test]
fn a_retry_only_counts_while_waiting_for_that_attempt() {
    let mut h = live(vec![]);
    h.send(HerdAction::Retry { attempt: 0 });
    assert_eq!((h.state.attempt, &h.state.link), (0, &Link::Live));

    h.send(disconnected(0, "eof"))
        .send(HerdAction::Retry { attempt: 7 });
    assert_eq!(h.state.attempt, 0);
    assert!(matches!(h.state.link, Link::Waiting { .. }));
}

#[test]
fn a_snapshot_while_waiting_changes_nothing() {
    let mut h = live(vec![]);
    h.send(disconnected(0, "eof")).send(HerdAction::Snapshot {
        attempt: 0,
        seq: 99,
        terminals: vec![terminal("t1", "live")],
    });

    assert!(h.state.terminals.is_empty());
    assert_eq!(h.state.seq, 10);
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

fn attach(terminal: &str, attempt: u32) -> Stream {
    Stream::Attach {
        socket: "/run/hc.sock".into(),
        terminal: terminal.into(),
        attempt,
    }
}

fn screen(line: &str) -> Screen {
    Screen {
        lines: vec![line.into()],
        cursor: (0, 0),
    }
}

#[test]
fn the_selected_live_terminal_is_attached_and_each_selection_gets_a_new_stream() {
    let mut h = live(vec![terminal("t1", "live"), terminal("t2", "live")]);
    assert_eq!(h.subscriptions(), [events(0), attach("t1", 1)]);

    h.send(HerdAction::Select("t2".into()));
    assert_eq!(h.subscriptions(), [events(0), attach("t2", 2)]);

    h.send(HerdAction::Select("t2".into()));
    assert_eq!(h.state.attachment.attempt, 2);
}

#[test]
fn nothing_is_attached_while_waiting_or_when_the_terminal_finished() {
    let finished = live(vec![terminal("t1", "finished")]);
    assert_eq!(finished.subscriptions(), [events(0)]);

    let mut h = live(vec![terminal("t1", "live")]);
    h.send(disconnected(0, "eof"));
    assert_eq!(h.subscriptions(), []);
}

#[test]
fn only_paints_from_the_current_attachment_reach_the_screen() {
    let mut h = live(vec![terminal("t1", "live"), terminal("t2", "live")]);
    let painted = |terminal: &str, attempt, line: &str| HerdAction::Painted {
        terminal: terminal.into(),
        attempt,
        screen: screen(line),
    };

    h.send(painted("t1", 1, "first"));
    assert_eq!(h.state.attachment.screen, Some(screen("first")));

    h.send(HerdAction::Select("t2".into()));
    assert_eq!(h.state.attachment.screen, None);
    h.send(painted("t1", 1, "late"))
        .send(painted("t2", 1, "wrong attempt"));
    assert_eq!(h.state.attachment.screen, None);

    h.send(painted("t2", 2, "second"));
    assert_eq!(h.state.attachment.screen, Some(screen("second")));
}

#[test]
fn a_detached_stream_stays_closed_until_reattach() {
    let mut h = live(vec![terminal("t1", "live")]);
    h.send(HerdAction::Reattach);
    assert_eq!(h.state.attachment.attempt, 1);

    h.send(HerdAction::Detached {
        terminal: "t1".into(),
        attempt: 1,
        reason: "exited, exit code 0".into(),
    });
    assert_eq!(
        h.state.attachment.ended.as_deref(),
        Some("exited, exit code 0")
    );
    assert_eq!(h.subscriptions(), [events(0)]);

    h.send(HerdAction::Reattach);
    assert_eq!(h.state.attachment.ended, None);
    assert_eq!(h.subscriptions(), [events(0), attach("t1", 2)]);
}

#[test]
fn a_detach_from_an_older_attachment_changes_nothing() {
    let mut h = live(vec![terminal("t1", "live")]);
    h.send(HerdAction::Detached {
        terminal: "t1".into(),
        attempt: 0,
        reason: "old".into(),
    });

    assert_eq!(h.state.attachment.ended, None);
}

#[test]
fn keys_are_typed_into_the_attached_terminal() {
    let mut h = live(vec![terminal("t1", "live")]);
    let key = |key: &str, text: Option<&str>| {
        HerdAction::Key(Key {
            key: key.into(),
            text: text.map(Into::into),
            ..Key::default()
        })
    };

    h.send(key("l", Some("l"))).send(key("shift", None));
    assert_eq!(
        h.take_effects(),
        [HerdEffect::Type {
            to: attach("t1", 1),
            bytes: b"l".to_vec(),
        }]
    );

    let mut finished = live(vec![terminal("t1", "finished")]);
    finished.send(key("l", Some("l")));
    assert!(finished.take_effects().is_empty());
}

mod protocols {
    use silon_core::{Exchange, Protocol};

    use super::*;
    use crate::protocols::{Attach, Events, SendText};
    use crate::wire::{Frame, Kind, attach_request, hello_request};

    fn feed<P: Protocol>(protocol: &mut P, bytes: &[u8]) -> Exchange<P::Action> {
        let mut exchange = Exchange::new();
        protocol.receive(bytes, &mut exchange);
        exchange
    }

    fn opened<P: Protocol>(protocol: &mut P) -> Vec<u8> {
        let mut exchange = Exchange::new();
        protocol.open(&mut exchange);
        exchange.take_written()
    }

    #[test]
    fn events_asks_for_the_stream_and_reads_lines_across_chunks() {
        let mut events = Events::new(0);
        assert_eq!(opened(&mut events), subscribe_request().into_bytes());

        let mut actions = Vec::new();
        for chunk in EVENTS.as_bytes().chunks(7) {
            actions.extend(feed(&mut events, chunk).take_actions());
        }
        let expected: Vec<_> = lines()
            .iter()
            .filter_map(|line| decode_stream_line(0, line))
            .collect();
        assert_eq!(actions, expected);
    }

    #[test]
    fn events_ends_on_a_reply_error() {
        let mut events = Events::new(4);
        opened(&mut events);
        let mut exchange = feed(
            &mut events,
            b"{\"error\":{\"message\":\"stopping\"},\"id\":1}\n{\"method\":\"heartbeat\"}\n",
        );

        assert_eq!(exchange.take_actions(), [disconnected(4, "stopping")]);
        assert!(exchange.is_finished());
        assert_eq!(
            Events::new(4).closed(std::io::ErrorKind::BrokenPipe.into()),
            Some(disconnected(4, "broken pipe"))
        );
    }

    #[test]
    fn send_writes_the_request_and_ends_with_the_reply() {
        let mut send = SendText::new("t1".into(), "hi".into());
        assert_eq!(opened(&mut send), send_request("t1", "hi").into_bytes());

        assert!(
            feed(&mut send, b"{\"id\":1,\"result\":")
                .take_actions()
                .is_empty()
        );
        let mut exchange = feed(&mut send, b"{\"turn\":3}}\n");
        assert_eq!(
            exchange.take_actions(),
            [HerdAction::Sent {
                terminal: "t1".into(),
                result: Ok(3),
            }]
        );
        assert!(exchange.is_finished());

        let closed = SendText::new("t1".into(), "hi".into())
            .closed(std::io::ErrorKind::ConnectionRefused.into());
        assert!(matches!(
            closed,
            Some(HerdAction::Sent { result: Err(_), .. })
        ));
    }

    const HELLO: &[u8] = b"{\"id\":1,\"jsonrpc\":\"2.0\",\"result\":{\"framed\":true}}\n";
    /// The start of a real first paint, then a prompt.
    const PAINT: &[u8] = b"\x18\x1b[!p\x1b[?1049l\x1b[H\x1b[2J$ ";

    fn data(channel: u32, payload: &[u8]) -> Vec<u8> {
        Frame::new(channel, Kind::Data, payload.to_vec()).encode()
    }

    fn attached_reply() -> Vec<u8> {
        data(
            0,
            br#"{"id":2,"jsonrpc":"2.0","result":{"channel":1,"automatic_input":true}}"#,
        )
    }

    fn credit(n: u32) -> Vec<u8> {
        Frame::new(1, Kind::Credit, n.to_le_bytes().to_vec()).encode()
    }

    fn screen_of(actions: &[HerdAction]) -> Option<&Screen> {
        actions.iter().rev().find_map(|action| match action {
            HerdAction::Painted { screen, .. } => Some(screen),
            _ => None,
        })
    }

    #[test]
    fn attach_says_hello_attaches_paints_and_gives_credit() {
        let mut attach = Attach::new("t1".into(), 5);
        assert_eq!(opened(&mut attach), hello_request().into_bytes());

        let mut exchange = feed(&mut attach, HELLO);
        let request = Frame::new(0, Kind::Data, attach_request("t1", SIZE)).encode();
        assert_eq!(exchange.take_written(), request);

        let echo = b"echo hi\r\nhi\r\n$ ";
        let server = [attached_reply(), data(1, PAINT), data(1, echo)].concat();
        let mut exchange = feed(&mut attach, &server);
        let painted = (PAINT.len() + echo.len()) as u32;
        assert_eq!(exchange.take_written(), credit(painted));
        let actions = exchange.take_actions();
        assert!(matches!(
            &actions[..],
            [HerdAction::Painted { terminal, attempt: 5, .. }] if terminal == "t1"
        ));
        let screen = screen_of(&actions).unwrap();
        assert_eq!(screen.lines[..3], ["$ echo hi", "hi", "$ "]);
        assert_eq!(screen.lines.len(), usize::from(SIZE.1));
        assert_eq!(screen.cursor, (2, 2));
    }

    #[test]
    fn attach_reads_frames_split_at_any_byte() {
        let mut attach = Attach::new("t1".into(), 0);
        opened(&mut attach);
        let server = [
            HELLO.to_vec(),
            attached_reply(),
            data(1, PAINT),
            data(1, b"ls\r\n"),
        ]
        .concat();

        let mut actions = Vec::new();
        let mut written = Vec::new();
        for byte in &server {
            let mut exchange = feed(&mut attach, std::slice::from_ref(byte));
            actions.extend(exchange.take_actions());
            written.extend(exchange.take_written());
        }

        assert_eq!(screen_of(&actions).unwrap().lines[0], "$ ls");
        let mut credits = 0;
        while let Some(frame) = Frame::take(&mut written) {
            if frame.is(Kind::Credit) {
                credits += u32::from_le_bytes(frame.payload[..].try_into().unwrap());
            }
        }
        assert_eq!(credits as usize, PAINT.len() + 4);
    }

    #[test]
    fn keys_typed_before_the_channel_is_known_follow_the_attach_reply() {
        let mut attach = Attach::new("t1".into(), 0);
        opened(&mut attach);
        let mut early = Exchange::new();
        attach.input(b"ab".to_vec(), &mut early);
        assert!(early.take_written().is_empty());

        feed(&mut attach, HELLO);
        let mut exchange = feed(&mut attach, &attached_reply());
        assert_eq!(exchange.take_written(), data(1, b"ab"));

        let mut later = Exchange::new();
        attach.input(b"\r".to_vec(), &mut later);
        assert_eq!(later.take_written(), data(1, b"\r"));
    }

    fn detached(reason: &str) -> HerdAction {
        HerdAction::Detached {
            terminal: "t1".into(),
            attempt: 0,
            reason: reason.into(),
        }
    }

    #[test]
    fn attach_ends_when_the_program_exits() {
        let mut attach = Attach::new("t1".into(), 0);
        opened(&mut attach);
        feed(&mut attach, &[HELLO.to_vec(), attached_reply()].concat());

        let close = Frame::new(
            1,
            Kind::Close,
            br#"{"reason":"exited","exit_code":0,"signal":null}"#.to_vec(),
        );
        let mut exchange = feed(&mut attach, &close.encode());

        assert_eq!(exchange.take_actions(), [detached("exited, exit code 0")]);
        assert!(exchange.is_finished());
    }

    #[test]
    fn attach_ends_on_an_error_reply() {
        let mut attach = Attach::new("t1".into(), 0);
        opened(&mut attach);
        feed(&mut attach, HELLO);
        let refused = data(
            0,
            br#"{"id":2,"error":{"code":-32000,"message":"no terminal \"t1\""}}"#,
        );
        let mut exchange = feed(&mut attach, &refused);
        assert_eq!(exchange.take_actions(), [detached("no terminal \"t1\"")]);
        assert!(exchange.is_finished());

        let mut attach = Attach::new("t1".into(), 0);
        let mut exchange = feed(
            &mut attach,
            b"{\"id\":1,\"error\":{\"message\":\"old core\"}}\n",
        );
        assert_eq!(exchange.take_actions(), [detached("old core")]);

        assert_eq!(
            Attach::new("t1".into(), 0).closed(std::io::ErrorKind::BrokenPipe.into()),
            Some(detached("broken pipe"))
        );
    }

    #[test]
    fn hello_and_attach_requests_carry_their_parameters() {
        let hello: serde_json::Value = serde_json::from_str(&hello_request()).unwrap();
        assert_eq!(hello["method"], "hello");
        assert_eq!(hello["params"]["framed"], true);

        let attach: serde_json::Value =
            serde_json::from_slice(&attach_request("t9", (80, 20))).unwrap();
        assert_eq!(attach["id"], 2);
        assert_eq!(attach["method"], "terminal.attach");
        assert_eq!(
            attach["params"],
            serde_json::json!({ "terminal": "t9", "cols": 80, "rows": 20 })
        );
    }

    #[test]
    fn take_lines_keeps_a_partial_line_for_later() {
        let mut pending = b"a\nb\nc".to_vec();
        assert_eq!(crate::wire::take_lines(&mut pending), ["a", "b"]);
        assert_eq!(pending, b"c");

        pending.extend_from_slice(b"\n");
        assert_eq!(crate::wire::take_lines(&mut pending), ["c"]);
        assert!(pending.is_empty());
    }

    #[test]
    fn while_attaching_only_a_data_reply_on_channel_zero_counts() {
        let mut attach = Attach::new("t1".into(), 0);
        opened(&mut attach);
        feed(&mut attach, HELLO);
        let reply = br#"{"id":2,"jsonrpc":"2.0","result":{"channel":1}}"#;

        let elsewhere = data(7, reply);
        let not_data = Frame::new(0, Kind::Close, reply.to_vec()).encode();
        let mut exchange = feed(&mut attach, &[elsewhere, not_data].concat());
        assert!(exchange.take_written().is_empty());

        let mut early = Exchange::new();
        attach.input(b"x".to_vec(), &mut early);
        assert!(early.take_written().is_empty(), "attached too soon");
    }

    #[test]
    fn attach_ignores_other_channels() {
        let mut attach = Attach::new("t1".into(), 0);
        opened(&mut attach);
        feed(&mut attach, &[HELLO.to_vec(), attached_reply()].concat());

        let mut exchange = feed(&mut attach, &data(7, b"not ours"));
        assert!(exchange.take_actions().is_empty());
        assert!(exchange.take_written().is_empty());
    }
}
