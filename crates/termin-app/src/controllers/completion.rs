//! Synchronous completion hook. Async jobs are returned for the Phase 3D worker.
use termin_editor::{complete, Candidate, CompletionContext, CompletionResult, EditorState};

/// Identity of one editor completion query. Worker replies must carry this id.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CompletionRequestId(pub u64);

/// Synchronous result together with the identity needed to reject stale jobs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionRequest {
    pub id: CompletionRequestId,
    pub result: CompletionResult,
}

/// Refresh the editor menu at its current cursor. Until the worker track is
/// available, preserve async jobs in the result for the caller to schedule.
pub fn refresh(
    editor: &mut EditorState,
    id: CompletionRequestId,
    context: &CompletionContext,
) -> CompletionRequest {
    let result = complete(&editor.text, editor.cursor, context);
    editor.menu = if result.candidates.is_empty() {
        None
    } else {
        Some(termin_editor::CompletionMenu::new(
            result.candidates.clone(),
        ))
    };
    CompletionRequest { id, result }
}

/// Merge results from worker-side generators into the visible menu. Sync
/// candidates retain their rank; async results append in stable worker order.
/// The editor crate's 3D worker hook can call this after a job completes.
pub fn merge_async(
    editor: &mut EditorState,
    request_id: CompletionRequestId,
    active_request_id: CompletionRequestId,
    candidates: Vec<Candidate>,
) -> bool {
    if request_id != active_request_id {
        return false;
    }
    // TODO(3D): schedule AsyncJob values on the shared worker and return them
    // here. Until then the caller may execute the bounded generator inline.
    let menu = editor
        .menu
        .get_or_insert_with(termin_editor::CompletionMenu::default);
    for candidate in candidates {
        if menu.candidates.len() >= termin_editor::complete::MAX_COMPLETION_CANDIDATES {
            break;
        }
        if !menu
            .candidates
            .iter()
            .any(|item| item.insert_text == candidate.insert_text)
        {
            menu.candidates.push(candidate);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(label: &str) -> Candidate {
        Candidate {
            label: label.to_string(),
            detail: None,
            insert_text: label.to_string(),
            score: 0,
        }
    }

    #[test]
    fn stale_async_completion_is_ignored() {
        let mut editor = EditorState::new();
        let active = CompletionRequestId(8);
        let stale = CompletionRequestId(7);
        editor.menu = Some(termin_editor::CompletionMenu::new(vec![candidate("sync")]));
        assert!(!merge_async(
            &mut editor,
            stale,
            active,
            vec![candidate("old")]
        ));
        assert_eq!(editor.menu.as_ref().unwrap().candidates.len(), 1);
        assert!(merge_async(
            &mut editor,
            active,
            active,
            vec![candidate("fresh")]
        ));
        assert_eq!(editor.menu.as_ref().unwrap().candidates[1].label, "fresh");
    }
}
