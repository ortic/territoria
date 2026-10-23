//! Matching table rows to features (or other rows) by key.
//!
//! Keys are compared as text. The match modes cover what goes wrong in
//! practice: case and stray spaces, leading zeros lost in a spreadsheet,
//! and finer keys rolled up onto coarser areas (`80331` → `80`).

use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MatchMode {
    /// Identical text.
    Exact,
    /// Ignoring case and surrounding/double spaces.
    Loose,
    /// Like `Loose`, and digit strings compared as numbers (`0812` = `812`).
    Numeric,
    /// The first `n` characters of the source key (after `Loose`) against
    /// the target key: finer codes onto coarser areas.
    Prefix(u8),
}

impl MatchMode {
    pub fn label(self) -> String {
        match self {
            MatchMode::Exact => "Exact".into(),
            MatchMode::Loose => "Ignore case and spaces".into(),
            MatchMode::Numeric => "Ignore leading zeros".into(),
            MatchMode::Prefix(n) => format!("First {n} characters"),
        }
    }
}

/// The key as compared under `mode`.
pub fn normalize(key: &str, mode: MatchMode) -> Cow<'_, str> {
    match mode {
        MatchMode::Exact => Cow::Borrowed(key),
        MatchMode::Loose => loose(key),
        MatchMode::Numeric => {
            let l = loose(key);
            if !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()) {
                let t = l.trim_start_matches('0');
                Cow::Owned(if t.is_empty() {
                    "0".into()
                } else {
                    t.to_owned()
                })
            } else {
                l
            }
        }
        MatchMode::Prefix(n) => {
            let l = loose(key);
            match l.char_indices().nth(n as usize) {
                Some((i, _)) => Cow::Owned(l[..i].to_owned()),
                None => l,
            }
        }
    }
}

fn loose(key: &str) -> Cow<'_, str> {
    let trimmed = key.trim();
    if trimmed.chars().any(|c| c.is_uppercase()) || trimmed.contains("  ") {
        Cow::Owned(
            trimmed
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase(),
        )
    } else {
        Cow::Borrowed(trimmed)
    }
}

/// How the target side is normalized: prefixes cut the source only.
fn target_mode(mode: MatchMode) -> MatchMode {
    match mode {
        MatchMode::Prefix(_) => MatchMode::Loose,
        m => m,
    }
}

/// Target keys (e.g. a layer's feature codes) for repeated matching.
#[derive(Debug, Clone)]
pub struct KeyIndex {
    mode: MatchMode,
    map: HashMap<String, u32>,
    /// Targets whose key occurs more than once (only the first is used).
    pub duplicates: Vec<u32>,
    pub targets: usize,
}

impl KeyIndex {
    pub fn new<'a>(keys: impl IntoIterator<Item = Option<&'a str>>, mode: MatchMode) -> KeyIndex {
        let mut map = HashMap::new();
        let mut duplicates = Vec::new();
        let mut targets = 0;
        for (i, k) in keys.into_iter().enumerate() {
            targets += 1;
            let Some(k) = k else { continue };
            let n = normalize(k, target_mode(mode)).into_owned();
            if n.is_empty() {
                continue;
            }
            if let std::collections::hash_map::Entry::Vacant(e) = map.entry(n) {
                e.insert(i as u32);
            } else {
                duplicates.push(i as u32);
            }
        }
        KeyIndex {
            mode,
            map,
            duplicates,
            targets,
        }
    }

    pub fn mode(&self) -> MatchMode {
        self.mode
    }

    pub fn find(&self, key: &str) -> Option<u32> {
        self.map.get(normalize(key, self.mode).as_ref()).copied()
    }
}

/// Result of matching source rows to targets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Matching {
    pub mode: MatchMode,
    /// Per source row, the target it belongs to.
    pub row_target: Vec<Option<u32>>,
    pub stats: MatchStats,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MatchStats {
    pub rows: usize,
    pub rows_empty: usize,
    pub rows_matched: usize,
    pub targets: usize,
    pub targets_matched: usize,
    /// Targets hit by more than one row.
    pub targets_shared: usize,
    /// A few source keys that found nothing, for the report.
    pub unmatched_samples: Vec<String>,
}

impl MatchStats {
    /// Share of rows with a key that found a target.
    pub fn row_rate(&self) -> f64 {
        let with_key = self.rows - self.rows_empty;
        if with_key == 0 {
            0.0
        } else {
            self.rows_matched as f64 / with_key as f64
        }
    }

    /// Share of targets that got at least one row.
    pub fn target_rate(&self) -> f64 {
        if self.targets == 0 {
            0.0
        } else {
            self.targets_matched as f64 / self.targets as f64
        }
    }
}

pub fn match_keys<'a>(
    source: impl IntoIterator<Item = Option<&'a str>>,
    index: &KeyIndex,
) -> Matching {
    let mut stats = MatchStats {
        targets: index.targets,
        ..Default::default()
    };
    let mut hits = vec![0u32; index.targets];
    let mut row_target = Vec::new();
    for key in source {
        stats.rows += 1;
        let key = key.map(str::trim).filter(|k| !k.is_empty());
        let target = match key {
            None => {
                stats.rows_empty += 1;
                None
            }
            Some(k) => {
                let t = index.find(k);
                match t {
                    Some(t) => {
                        stats.rows_matched += 1;
                        hits[t as usize] += 1;
                    }
                    None if stats.unmatched_samples.len() < 20 => {
                        stats.unmatched_samples.push(k.to_owned())
                    }
                    None => {}
                }
                t
            }
        };
        row_target.push(target);
    }
    stats.targets_matched = hits.iter().filter(|&&h| h > 0).count();
    stats.targets_shared = hits.iter().filter(|&&h| h > 1).count();
    Matching {
        mode: index.mode,
        row_target,
        stats,
    }
}

/// Tries the match modes from strict to loose and returns the first that
/// matches well, else the best. Prefix roll-ups are tried when the target
/// keys are shorter than the source keys.
pub fn best_match<'a>(source: &[Option<&'a str>], targets: &[Option<&'a str>]) -> Matching {
    let mut modes = vec![MatchMode::Exact, MatchMode::Loose, MatchMode::Numeric];
    let len = |keys: &[Option<&str>]| {
        let mut lens: Vec<usize> = keys
            .iter()
            .flatten()
            .map(|k| k.trim().chars().count())
            .collect();
        lens.sort_unstable();
        lens.get(lens.len() / 2).copied().unwrap_or(0)
    };
    let (src_len, tgt_len) = (len(source), len(targets));
    if tgt_len > 0 && src_len > tgt_len && tgt_len < 256 {
        modes.push(MatchMode::Prefix(tgt_len as u8));
    }
    let mut best: Option<Matching> = None;
    for mode in modes {
        let index = KeyIndex::new(targets.iter().copied(), mode);
        let m = match_keys(source.iter().copied(), &index);
        if m.stats.row_rate() >= 0.98 {
            return m;
        }
        if best
            .as_ref()
            .is_none_or(|b| m.stats.rows_matched > b.stats.rows_matched)
        {
            best = Some(m);
        }
    }
    best.expect("at least one mode")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize(" AB  12 ", MatchMode::Loose), "ab 12");
        assert_eq!(normalize("00812", MatchMode::Numeric), "812");
        assert_eq!(normalize("000", MatchMode::Numeric), "0");
        assert_eq!(normalize("A01", MatchMode::Numeric), "a01");
        assert_eq!(normalize("80331", MatchMode::Prefix(2)), "80");
        assert_eq!(normalize("8", MatchMode::Prefix(2)), "8");
    }

    #[test]
    fn matches_with_stats() {
        let targets = [Some("01067"), Some("10115"), Some("80331"), None];
        let index = KeyIndex::new(targets, MatchMode::Numeric);
        let m = match_keys(
            [
                Some("1067"),
                Some("1067"),
                Some("99999"),
                None,
                Some("80331"),
            ],
            &index,
        );
        assert_eq!(m.row_target, vec![Some(0), Some(0), None, None, Some(2)]);
        assert_eq!(m.stats.rows_matched, 3);
        assert_eq!(m.stats.rows_empty, 1);
        assert_eq!(m.stats.targets_matched, 2);
        assert_eq!(m.stats.targets_shared, 1);
        assert_eq!(m.stats.unmatched_samples, vec!["99999"]);
        assert!((m.stats.row_rate() - 0.75).abs() < 1e-9);
    }

    #[test]
    fn best_match_finds_leading_zero_and_prefix() {
        let targets = [Some("01067"), Some("10115")];
        let m = best_match(&[Some("1067"), Some("10115")], &targets);
        assert_eq!(m.mode, MatchMode::Numeric);
        assert_eq!(m.stats.rows_matched, 2);

        let regions = [Some("80"), Some("10")];
        let m = best_match(&[Some("80331"), Some("10115"), Some("80999")], &regions);
        assert_eq!(m.mode, MatchMode::Prefix(2));
        assert_eq!(m.row_target, vec![Some(0), Some(1), Some(0)]);
    }
}
