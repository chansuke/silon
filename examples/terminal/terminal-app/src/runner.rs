use std::path::PathBuf;

use silon_core::{BoxFuture, BoxStream, EffectRunner, Mailboxes};
use terminal_core::{Input, Pty, TermAction, TermEffect, Terminal};

use crate::session;

/// Runs each session on a thread of its own and hands it what the feature
/// posts.
pub struct TermRunner {
    inputs: Mailboxes<Pty, Input>,
    /// Where the `xterm-ghostty` terminfo entry was written, if it could be.
    terminfo: Option<PathBuf>,
}

impl TermRunner {
    pub fn new() -> Self {
        let terminfo = ghostty_vt::terminfo::dir()
            .inspect_err(|err| eprintln!("no ghostty terminfo, using xterm-256color: {err}"))
            .ok();
        Self {
            inputs: Mailboxes::new(),
            terminfo,
        }
    }
}

impl EffectRunner<Terminal> for TermRunner {
    fn run(&self, effect: TermEffect) -> BoxFuture<'static, Option<TermAction>> {
        match effect {
            TermEffect::Input { to, input } => {
                // Posted here, not in the future, so keys keep their order.
                // A session that has just ended has nowhere to put them.
                let _ = self.inputs.post(&to, input);
                Box::pin(async { None })
            }
        }
    }

    fn subscribe(&self, pty: Pty) -> BoxStream<'static, TermAction> {
        let inputs = self.inputs.open(pty.clone());
        session::start(pty, inputs, self.terminfo.clone())
    }
}
