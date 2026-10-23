//! Undo and redo as project snapshots (cheap: heavy data is shared).

use std::time::{Duration, Instant};

use crate::Project;

const LIMIT: usize = 200;
/// Changes with the same label this close together are one undo step
/// (dragging a slider, typing a name).
const COALESCE: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub struct History {
    undo: Vec<(String, Project)>,
    redo: Vec<(String, Project)>,
    last: Option<(String, Instant)>,
    /// Changes since the last save.
    pub dirty: bool,
}

impl History {
    /// Records `before`, the state ahead of a change named `label`.
    pub fn push(&mut self, label: impl Into<String>, before: Project) {
        let label = label.into();
        let now = Instant::now();
        let merge = self
            .last
            .as_ref()
            .is_some_and(|(l, t)| *l == label && now.duration_since(*t) < COALESCE)
            && self.undo.last().is_some_and(|(l, _)| *l == label);
        if !merge {
            self.undo.push((label.clone(), before));
            if self.undo.len() > LIMIT {
                self.undo.remove(0);
            }
        }
        self.last = Some((label, now));
        self.redo.clear();
        self.dirty = true;
    }

    /// Starts a new undo step even for the same label (e.g. a new paint stroke).
    pub fn seal(&mut self) {
        self.last = None;
    }

    /// Restores the previous state; returns what was undone.
    pub fn undo(&mut self, project: &mut Project) -> Option<String> {
        self.last = None;
        let (label, before) = self.undo.pop()?;
        let after = std::mem::replace(project, before);
        self.redo.push((label.clone(), after));
        self.dirty = true;
        Some(label)
    }

    pub fn redo(&mut self, project: &mut Project) -> Option<String> {
        self.last = None;
        let (label, after) = self.redo.pop()?;
        let before = std::mem::replace(project, after);
        self.undo.push((label.clone(), before));
        self.dirty = true;
        Some(label)
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|(l, _)| l.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|(l, _)| l.as_str())
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last = None;
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_redo() {
        let mut h = History::default();
        let mut p = Project::new();
        h.push("rename", p.clone());
        p.title = "B".into();
        assert_eq!(h.undo(&mut p).as_deref(), Some("rename"));
        assert_eq!(p.title, "Untitled");
        assert_eq!(h.redo(&mut p).as_deref(), Some("rename"));
        assert_eq!(p.title, "B");
        assert!(h.redo(&mut p).is_none());
    }

    #[test]
    fn coalesces_repeated_changes() {
        let mut h = History::default();
        let mut p = Project::new();
        for i in 0..5 {
            h.push("opacity", p.clone());
            p.title = format!("{i}");
        }
        h.seal();
        h.push("opacity", p.clone());
        p.title = "last".into();
        h.undo(&mut p);
        assert_eq!(p.title, "4");
        h.undo(&mut p);
        assert_eq!(p.title, "Untitled");
        assert!(h.undo(&mut p).is_none());
    }
}
