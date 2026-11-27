// How silon-gpui's Store keeps a feature's subscriptions in step with its state.
//
// Scope: the Store reconciles the running subscriptions with
// `Feature::subscriptions(state)` after every dispatch, streams that produce
// items and end in the background, store release, and the history that
// introspection replays. The feature is a stream that reconnects: while it is
// enabled it subscribes to `attempt`, and the stream's last item `Ended`
// moves it to the next attempt, which is a new subscription.
//
// The Store forwards each stream's items to the main thread in the order the
// stream produced them, and drops the ones not yet dispatched when the
// subscription stops. A stream that ends is not restarted while the state
// still asks for it; the feature restarts it by asking for a different key.
//
// Checked properties:
// - reconciled: after every step, the running subscriptions are exactly the
//   ones the state asks for (none once the store is released).
// - stoppedAreEmpty: a stopped subscription keeps no undelivered items, so
//   starting the same key again begins a fresh stream.
// - replayMatches, historyBounded: replaying the history gives the live state,
//   so the replayed state asks for the running subscriptions too.
// - inOrder, noStaleTicks: without an agent, each attempt's items reach update
//   in the order they were produced, and no item reaches update after its
//   subscription stopped. An agent can dispatch any item itself;
//   `agentDeliversStaleTickTest` demonstrates this trace.
module SubscriptionProtocol {
  pure val MAX_ATTEMPT = 2
  pure val MAX_ITEMS = 2
  pure val CAPACITY = 2
  pure val KEYS = 0.to(MAX_ATTEMPT)

  type Tick = { key: int, seq: int }

  type Action =
    | Enable
    | Disable
    | Item(Tick)
    | Ended(int)

  type State = {
    enabled: bool,
    attempt: int,
    // Sequence number of the last item of `attempt`, -1 before the first.
    lastSeq: int,
    outOfOrder: bool,
    staleTicks: int,
  }

  pure val initialState: State =
    { enabled: false, attempt: 0, lastSeq: -1, outOfOrder: false, staleTicks: 0 }

  // Feature::update.
  pure def update(s: State, a: Action): State =
    match a {
      | Enable => { ...s, enabled: true }
      | Disable => { ...s, enabled: false }
      | Item(t) =>
          if (s.enabled and t.key == s.attempt)
            { ...s, lastSeq: t.seq, outOfOrder: s.outOfOrder or t.seq != s.lastSeq + 1 }
          else { ...s, staleTicks: s.staleTicks + 1 }
      | Ended(k) =>
          if (s.enabled and k == s.attempt and s.attempt < MAX_ATTEMPT)
            { ...s, attempt: s.attempt + 1, lastSeq: -1 }
          else s
    }

  // Feature::subscriptions.
  pure def subscriptions(s: State): Set[int] =
    if (s.enabled) Set(s.attempt) else Set()

  // ActionHistory::record, as in StoreProtocol.
  pure def record(b: State, h: List[Action], a: Action): (State, List[Action]) = {
    val recorded = h.append(a)
    if (recorded.length() > CAPACITY) (update(b, recorded[0]), recorded.slice(1, recorded.length()))
    else (b, recorded)
  }

  // Live store.
  var state: State
  var released: bool
  // Subscriptions the store runs.
  var running: Set[int]
  // Per key: items the stream produced that the main thread has not
  // dispatched yet, how many items it produced, and whether it ended.
  var buffer: int -> List[Action]
  var produced: int -> int
  var ended: Set[int]
  // Introspection history.
  var base: State
  var history: List[Action]

  pure val emptyBuffers: int -> List[Action] = KEYS.mapBy(_ => List())
  pure val zeroCounts: int -> int = KEYS.mapBy(_ => 0)

  action init = all {
    state' = initialState,
    released' = false,
    running' = subscriptions(initialState),
    buffer' = emptyBuffers,
    produced' = zeroCounts,
    ended' = Set(),
    base' = initialState,
    history' = List(),
  }

  // Stops the subscriptions that `wanted` leaves out, dropping what their
  // streams left undelivered, and starts the new ones from scratch.
  // `buf` is the buffer map after the step's own change.
  action reconcile(wanted: Set[int], buf: int -> List[Action]): bool = all {
    running' = wanted,
    buffer' = KEYS.mapBy(k => if (wanted.contains(k) and running.contains(k)) buf.get(k) else List()),
    produced' = KEYS.mapBy(k => if (wanted.contains(k) and running.contains(k)) produced.get(k) else 0),
    ended' = ended.intersect(wanted).intersect(running),
  }

  // Store::dispatch: update, record, then reconcile.
  action dispatchWith(a: Action, buf: int -> List[Action]): bool = {
    val after = update(state, a)
    val recorded = record(base, history, a)
    all {
      not(released),
      state' = after,
      released' = released,
      base' = recorded._1,
      history' = recorded._2,
      reconcile(subscriptions(after), buf),
    }
  }

  action dispatch(a: Action): bool = dispatchWith(a, buffer)

  // The UI can turn the stream on and off.
  action uiDispatch = any { dispatch(Enable), dispatch(Disable) }

  // An agent can send any action over HTTP, including items of any key.
  action agentDispatch = any {
    uiDispatch,
    nondet k = KEYS.oneOf()
    nondet n = 0.to(MAX_ITEMS).oneOf()
    any { dispatch(Item({ key: k, seq: n })), dispatch(Ended(k)) },
  }

  action unchangedStore = all {
    state' = state,
    released' = released,
    running' = running,
    base' = base,
    history' = history,
  }

  // A running stream produces its next item in the background.
  action produceItem = {
    nondet k = running.oneOf()
    all {
      not(ended.contains(k)),
      produced.get(k) < MAX_ITEMS,
      buffer' = buffer.set(k, buffer.get(k).append(Item({ key: k, seq: produced.get(k) }))),
      produced' = produced.set(k, produced.get(k) + 1),
      ended' = ended,
      unchangedStore,
    }
  }

  // A running stream yields its last item and ends.
  action produceEnd = {
    nondet k = running.oneOf()
    all {
      not(ended.contains(k)),
      buffer' = buffer.set(k, buffer.get(k).append(Ended(k))),
      produced' = produced,
      ended' = ended.union(Set(k)),
      unchangedStore,
    }
  }

  // The main thread dispatches the oldest undelivered item of a stream.
  action deliver = {
    nondet k = running.oneOf()
    val items = buffer.get(k)
    all {
      items.length() > 0,
      dispatchWith(items[0], buffer.set(k, items.slice(1, items.length()))),
    }
  }

  // The window closes: the store and every subscription are dropped.
  action release = all {
    not(released),
    released' = true,
    state' = state,
    base' = base,
    history' = history,
    running' = Set(),
    buffer' = emptyBuffers,
    produced' = zeroCounts,
    ended' = Set(),
  }

  action idle = all {
    released,
    unchangedStore,
    buffer' = buffer,
    produced' = produced,
    ended' = ended,
  }

  action streamStep = any { produceItem, produceEnd, deliver }

  // Without an agent.
  action uiStep = any { uiDispatch, streamStep, release, idle }

  // With an agent.
  action step = any { agentDispatch, streamStep, release, idle }

  val reconciled = running == (if (released) Set() else subscriptions(state))

  // A stopped subscription keeps nothing that a later start could deliver.
  val stoppedAreEmpty = KEYS.forall(k =>
    running.contains(k) or (buffer.get(k) == List() and produced.get(k) == 0 and not(ended.contains(k))))

  val replayMatches = history.foldl(base, (s, a) => update(s, a)) == state

  val historyBounded = history.length() <= CAPACITY

  val inOrder = not(state.outOfOrder)

  val noStaleTicks = state.staleTicks == 0

  // A stream that reconnects: enable, end attempt 0, and attempt 1 runs.
  run reconnectsTest = {
    init
      .then(dispatch(Enable))
      .expect(running == Set(0))
      .then(produceItem)
      .then(produceEnd)
      .then(deliver)
      .expect(state.lastSeq == 0)
      .then(deliver)
      .expect(running == Set(1) and state.attempt == 1 and state.lastSeq == -1)
  }

  // Disabling drops the item the stream produced but did not deliver.
  run disableDropsUndeliveredTest = {
    init
      .then(dispatch(Enable))
      .then(produceItem)
      .then(dispatch(Disable))
      .expect(running == Set() and buffer.get(0) == List())
      .then(dispatch(Enable))
      .expect(running == Set(0) and produced.get(0) == 0 and noStaleTicks)
  }

  // Deterministic witness that an agent can dispatch an item of a stopped
  // subscription, which update then counts as stale.
  run agentDeliversStaleTickTest = {
    init
      .then(dispatch(Item({ key: 0, seq: 0 })))
      .expect(not(noStaleTicks))
  }
}
