// The Store protocol of silon-gpui with the counter feature of the examples.
//
// The GPUI main thread applies one action at a time (`dispatch`). Effects run
// in the background and finish in any order; each follow-up action is
// dispatched back on the main thread, or dropped once the store is released.
// With introspection attached, the store records every dispatched action in
// a history of at most CAPACITY entries, folding the oldest into `base`.
//
// Checked properties:
// - replayMatches, historyBounded: under every interleaving, including an
//   agent that dispatches any action over HTTP, replaying the history from
//   the base gives the live state. This is what `GET /replay` reports as
//   `matches_current`.
// - oneAddInFlight: with actions from the UI and from effects only, at most
//   one delayed add is in flight and `loading` says whether one is.
//   An agent breaks this by dispatching `AsyncAddCompleted` itself while an
//   add is in flight; `quint verify --invariant=oneAddInFlight` (with the
//   default `step`) shows the trace.
module StoreProtocol {
  // Bounds of the model. The Rust code saturates at i32::MIN and i32::MAX;
  // small bounds reach saturation within a few steps.
  pure val MIN = -2
  pure val MAX = 2
  pure val DELTAS = Set(-3, 1)
  pure val CAPACITY = 2

  type CounterState = { count: int, loading: bool }

  type Action =
    | Increment
    | Decrement
    | RequestAsyncAdd(int)
    | AsyncAddCompleted(int)

  pure def saturate(n: int): int = if (n > MAX) MAX else if (n < MIN) MIN else n

  // CounterFeature::update: the new state and the deltas of started adds.
  pure def update(s: CounterState, a: Action): (CounterState, List[int]) =
    match a {
      | Increment => ({ ...s, count: saturate(s.count + 1) }, [])
      | Decrement => ({ ...s, count: saturate(s.count - 1) }, [])
      | RequestAsyncAdd(d) =>
          if (s.loading) (s, []) else ({ ...s, loading: true }, [d])
      | AsyncAddCompleted(d) => ({ count: saturate(s.count + d), loading: false }, [])
    }

  pure def applyAction(s: CounterState, a: Action): CounterState = update(s, a)._1

  pure def removeAt(l: List[int], i: int): List[int] =
    l.slice(0, i).concat(l.slice(i + 1, l.length()))

  // ActionHistory::record: append, then fold the oldest entry into the base
  // when the history is over capacity.
  pure def record(b: CounterState, h: List[Action], a: Action): (CounterState, List[Action]) = {
    val recorded = h.append(a)
    if (recorded.length() > CAPACITY) (applyAction(b, recorded[0]), recorded.slice(1, recorded.length()))
    else (b, recorded)
  }

  // Live store.
  var state: CounterState
  // Deltas of the delayed adds whose effects have not finished.
  var inFlight: List[int]
  var released: bool
  // Introspection history.
  var base: CounterState
  var history: List[Action]

  action init = all {
    state' = { count: 0, loading: false },
    inFlight' = [],
    released' = false,
    base' = { count: 0, loading: false },
    history' = [],
  }

  // Store::dispatch, with the recording of ActionHistory. `pending` are the
  // other effects still in flight.
  action dispatchWith(a: Action, pending: List[int]): bool = {
    val result = update(state, a)
    val recorded = record(base, history, a)
    all {
      not(released),
      state' = result._1,
      inFlight' = pending.concat(result._2),
      released' = released,
      base' = recorded._1,
      history' = recorded._2,
    }
  }

  action dispatch(a: Action): bool = dispatchWith(a, inFlight)

  // The UI can send the actions of its buttons.
  action uiDispatch = any {
    dispatch(Increment),
    dispatch(Decrement),
    nondet d = DELTAS.oneOf()
    dispatch(RequestAsyncAdd(d)),
  }

  // An agent can send any action over HTTP.
  action agentDispatch = {
    nondet d = DELTAS.oneOf()
    any {
      dispatch(Increment),
      dispatch(Decrement),
      dispatch(RequestAsyncAdd(d)),
      dispatch(AsyncAddCompleted(d)),
    }
  }

  // A delayed add finishes. Its follow-up is dispatched while the store is
  // alive and dropped after.
  action finish(i: int): bool = {
    val rest = removeAt(inFlight, i)
    if (released) all {
      inFlight' = rest,
      state' = state,
      released' = released,
      base' = base,
      history' = history,
    } else dispatchWith(AsyncAddCompleted(inFlight[i]), rest)
  }

  action effectFinishes = all {
    inFlight.length() > 0,
    nondet i = inFlight.indices().oneOf()
    finish(i),
  }

  // The window closes and the store entity is released.
  action release = all {
    not(released),
    released' = true,
    state' = state,
    inFlight' = inFlight,
    base' = base,
    history' = history,
  }

  action idle = all {
    state' = state,
    inFlight' = inFlight,
    released' = released,
    base' = base,
    history' = history,
  }

  // Without an agent.
  action uiStep = any { uiDispatch, effectFinishes, release, idle }

  // With an agent.
  action step = any { uiDispatch, agentDispatch, effectFinishes, release, idle }

  val replayMatches = history.foldl(base, (s, a) => applyAction(s, a)) == state

  val historyBounded = history.length() <= CAPACITY

  val countInBounds = MIN <= state.count and state.count <= MAX

  val oneAddInFlight =
    released or (inFlight.length() <= 1 and state.loading == (inFlight.length() == 1))
}
