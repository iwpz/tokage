//! Read-only replay of completed turns and persisted accepted COMPACT/FRESH anchors.
use crate::inputs::Observation;
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct ResetAction {
    pub action_id: i64,
    pub attempt_id: String,
    pub new_attempt_id: Option<String>,
    pub applied_at_ms: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cooldown {
    pub known: bool,
    pub turns_since_last_compact: u32,
    pub remaining_turns: u32,
    pub last_reset_action_id: Option<i64>,
    pub source: &'static str,
}
/// `samples` are already isolated, ordered and deduplicated by resolve_inputs.
/// A preview/query is never a turn. Only successful durable completion advances the count.
pub fn replay(samples: &[&Observation], actions: &[ResetAction]) -> Cooldown {
    let index = |id: &str| samples.iter().position(|s| s.id == id);
    let reset = actions
        .iter()
        .filter_map(|a| {
            let anchor = a
                .new_attempt_id
                .as_deref()
                .and_then(index)
                .or_else(|| index(&a.attempt_id))?;
            Some((a, anchor))
        })
        .max_by_key(|(a, anchor)| (a.applied_at_ms, *anchor, a.action_id));
    let count = samples
        .iter()
        .enumerate()
        .filter(|(i, s)| {
            s.completed
                && reset.is_none_or(|(a, anchor)| {
                    *i > anchor && s.ended_at.saturating_mul(1000) > a.applied_at_ms
                })
        })
        .count()
        .min(u32::MAX as usize) as u32;
    Cooldown {
        known: true,
        turns_since_last_compact: count,
        remaining_turns: 2u32.saturating_sub(count),
        last_reset_action_id: reset.map(|(a, _)| a.action_id),
        source: if reset.is_some() {
            "COMPLETED_TURNS_AFTER_PERSISTED_ACCEPTED_ACTION"
        } else {
            "COMPLETED_WINDOW_TURNS_NO_APPLIED_RESET"
        },
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sample(id: &str, ended_at: u64, completed: bool) -> Observation {
        Observation {
            id: id.into(),
            window: "w".into(),
            provider: "p".into(),
            model: "m".into(),
            ended_at,
            completed,
            context_tokens: None,
            output_tokens: None,
            api_calls: None,
            visible_tokens: 0.,
        }
    }
    #[test]
    fn completed_turns_advance_once_and_acceptance_resets_then_continue_advances() {
        let turns = [
            sample("a", 10, true),
            sample("failed", 11, false),
            sample("b", 12, true),
            sample("c", 14, true),
        ];
        let rows: Vec<_> = turns.iter().collect();
        assert_eq!(replay(&rows, &[]).turns_since_last_compact, 3);
        let reset = ResetAction {
            action_id: 1,
            attempt_id: "a".into(),
            new_attempt_id: None,
            applied_at_ms: 10_500,
        };
        assert_eq!(
            replay(&rows[..1], &[reset.clone()]).turns_since_last_compact,
            0
        );
        assert_eq!(replay(&rows[..3], &[reset.clone()]).remaining_turns, 1);
        assert_eq!(replay(&rows, &[reset.clone()]).remaining_turns, 0);
        assert_eq!(
            replay(&rows, &[reset.clone(), reset]).turns_since_last_compact,
            2
        );
    }
    #[test]
    fn fresh_target_is_the_reset_anchor_and_late_acceptance_does_not_count_old_turns() {
        let turns = [sample("new", 20, true), sample("next", 22, true)];
        let rows: Vec<_> = turns.iter().collect();
        let reset = ResetAction {
            action_id: 2,
            attempt_id: "old-window".into(),
            new_attempt_id: Some("new".into()),
            applied_at_ms: 19_000,
        };
        assert_eq!(replay(&rows, &[reset]).turns_since_last_compact, 1);
        let late = ResetAction {
            action_id: 3,
            attempt_id: "new".into(),
            new_attempt_id: None,
            applied_at_ms: 23_000,
        };
        assert_eq!(replay(&rows, &[late]).turns_since_last_compact, 0);
    }
    #[test]
    fn foreign_actions_and_ambiguous_same_second_completions_do_not_unlock_cooldown() {
        let turns = [sample("a", 10, true), sample("b", 10, true)];
        let rows: Vec<_> = turns.iter().collect();
        let foreign = ResetAction {
            action_id: 1,
            attempt_id: "other".into(),
            new_attempt_id: None,
            applied_at_ms: 10_001,
        };
        assert_eq!(replay(&rows, &[foreign]).turns_since_last_compact, 2);
        let own = ResetAction {
            action_id: 2,
            attempt_id: "a".into(),
            new_attempt_id: None,
            applied_at_ms: 10_001,
        };
        assert_eq!(replay(&rows, &[own]).turns_since_last_compact, 0);
    }
}
