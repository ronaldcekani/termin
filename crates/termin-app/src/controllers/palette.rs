//! Palette query parsing and bounded fuzzy ranking (Phase 3C).
use crate::{
    action::{Action, ActionInfo},
    frecency::Frecency,
};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteMode {
    Actions,
    Goto,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteFilter {
    All,
    Actions,
    Tabs,
    Workspaces,
    Directories,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RankedAction {
    pub action: Action,
    pub title: &'static str,
    pub score: i32,
}

/// Stateful input/query portion of the palette. UI remains a pure renderer.
#[derive(Debug)]
pub struct PaletteState {
    pub mode: PaletteMode,
    pub query: String,
    pub selected: usize,
    pub open: bool,
}
impl Default for PaletteState {
    fn default() -> Self {
        Self {
            mode: PaletteMode::Actions,
            query: String::new(),
            selected: 0,
            open: false,
        }
    }
}
impl PaletteState {
    pub fn open(&mut self, mode: PaletteMode) {
        self.mode = mode;
        self.query.clear();
        self.selected = 0;
        self.open = true;
    }
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
    }
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.selected = 0;
    }
    pub fn filter(&self) -> (PaletteFilter, &str) {
        match self.query.as_bytes().first() {
            Some(b'>') => (PaletteFilter::Actions, &self.query[1..]),
            Some(b'@') => (PaletteFilter::Tabs, &self.query[1..]),
            Some(b'#') => (PaletteFilter::Workspaces, &self.query[1..]),
            Some(b'/') => (PaletteFilter::Directories, &self.query[1..]),
            _ => (
                if self.mode == PaletteMode::Actions {
                    PaletteFilter::Actions
                } else {
                    PaletteFilter::All
                },
                &self.query,
            ),
        }
    }
    pub fn rank_actions(
        &self,
        items: &[ActionInfo],
        frequent: &Frecency,
        now: Instant,
        limit: usize,
    ) -> Vec<RankedAction> {
        let (_, query) = self.filter();
        let mut ranked = Vec::with_capacity(items.len());
        for item in items {
            if let Some(s) =
                fuzzy_score(query, item.title).or_else(|| fuzzy_score(query, item.action.name()))
            {
                let boost = (frequent.score(item.action.name(), now) * 20.0) as i32;
                ranked.push(RankedAction {
                    action: item.action.clone(),
                    title: item.title,
                    score: s + boost,
                });
            }
        }
        ranked.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.title.cmp(b.title)));
        ranked.truncate(limit);
        ranked
    }
}

/// Subsequence fuzzy score with word-boundary and compactness bonuses.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let mut q = query.chars();
    let mut wanted = q.next()?;
    let query_len = query.chars().count();
    let candidate_len = candidate.chars().count();
    let mut qi = 1;
    let mut last: Option<usize> = None;
    let mut score = 0;
    let mut prev = None;
    for (i, ch) in candidate.chars().enumerate() {
        if ch.eq_ignore_ascii_case(&wanted) {
            score += 10;
            if i == 0
                || prev.is_some_and(|p: char| {
                    !p.is_alphanumeric() || (p.is_ascii_lowercase() && ch.is_ascii_uppercase())
                })
            {
                score += 8;
            }
            if let Some(p) = last {
                if i == p + 1 {
                    score += 7;
                } else {
                    score -= ((i - p - 1).min(8)) as i32;
                }
            }
            last = Some(i);
            qi += 1;
            if qi > query_len {
                return Some(score - candidate_len as i32 / 8);
            }
            wanted = q.next()?;
        }
        prev = Some(ch);
    }
    None
}

/// Palette actions are executed by the app's overlay dispatcher.
pub fn handle_action(state: &mut PaletteState, action: &Action) -> bool {
    match action {
        Action::CommandPalette => state.open(PaletteMode::Actions),
        Action::GotoPalette => state.open(PaletteMode::Goto),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fuzzy_is_subsequence_and_prefixes_work() {
        assert!(fuzzy_score("splr", "Split Right").is_some());
        assert!(fuzzy_score("s", "Split Right").is_some());
        assert!(fuzzy_score("sx", "Split Right").is_none());
        assert!(fuzzy_score("xyz", "Split Right").is_none());
        let mut p = PaletteState::default();
        p.set_query("#work");
        assert_eq!(p.filter(), (PaletteFilter::Workspaces, "work"));
    }
}
