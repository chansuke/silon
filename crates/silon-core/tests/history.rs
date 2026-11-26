//! Property tests for `History` over longer runs than the Kani harness covers.

use proptest::prelude::*;
use silon_core::{History, ReplayError};

/// Order-sensitive, so a replay in the wrong order shows.
fn apply(state: &mut u64, entry: &u8) -> Result<(), String> {
    *state = state.wrapping_mul(31).wrapping_add(u64::from(*entry));
    Ok(())
}

proptest! {
    /// Each step is an action, stored or not (`None`). Replaying through any
    /// listed seq gives the state right after that action.
    #[test]
    fn replay_matches_the_states_that_were_recorded(
        capacity in 0_usize..8,
        base in any::<u64>(),
        steps in prop::collection::vec((any::<u8>(), prop::bool::weighted(0.9)), 0..40),
    ) {
        let mut history = History::new(base, capacity);
        let mut states = vec![base];
        let mut state = base;
        for &(entry, storable) in &steps {
            apply(&mut state, &entry).unwrap();
            history.record(storable.then_some(entry), &state, apply);
            states.push(state);
        }

        let count = steps.len() as u64;
        let listed = history.entries().count();
        prop_assert!(listed <= capacity);
        prop_assert_eq!(history.first_seq() + listed as u64, count);
        prop_assert_eq!(history.replay(None, apply), Ok(state));
        for seq in 0..count + 2 {
            let replayed = history.replay(Some(seq), apply);
            if seq < history.first_seq() {
                let oldest = history.first_seq();
                prop_assert_eq!(replayed, Err(ReplayError::Dropped { seq, oldest }));
            } else if seq >= count {
                prop_assert_eq!(replayed, Err(ReplayError::NotRecorded { seq }));
            } else {
                prop_assert_eq!(replayed, Ok(states[seq as usize + 1]));
            }
        }
        for (seq, entry) in history.entries() {
            prop_assert_eq!(Some(*entry), steps.get(seq as usize).map(|step| step.0));
        }
    }
}
