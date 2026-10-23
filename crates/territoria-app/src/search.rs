//! Finding areas, points and territories by key or name.

use territoria_model::{LayerId, PlanId, Project, TerritoryId};

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    Feature(LayerId, u32),
    Territory(PlanId, TerritoryId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub label: String,
    /// Where it was found: the layer or plan.
    pub detail: String,
    pub target: Target,
}

/// Up to `limit` matches for `query`: exact keys first, then names and
/// keys starting with it, then those containing it.
pub fn search(project: &Project, query: &str, limit: usize) -> Vec<Hit> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    // (rank, hit): 0 exact, 1 prefix, 2 contains.
    let mut found: Vec<(u8, Hit)> = Vec::new();
    let rank = |text: &str| -> Option<u8> {
        let t = text.to_lowercase();
        if t == q {
            Some(0)
        } else if t.starts_with(&q) {
            Some(1)
        } else if t.contains(&q) {
            Some(2)
        } else {
            None
        }
    };
    for plan in &project.plans {
        for t in &plan.territories {
            if let Some(r) = [rank(&t.name), rank(&t.code)].into_iter().flatten().min() {
                found.push((
                    r,
                    Hit {
                        label: t.name.clone(),
                        detail: format!("territory · {}", plan.name),
                        target: Target::Territory(plan.id, t.id),
                    },
                ));
            }
        }
    }
    for layer in &project.layers {
        let names = layer
            .name_column
            .as_deref()
            .and_then(|c| layer.attributes.column(c));
        for f in 0..layer.len() {
            let key = layer.key(f);
            let name = names.map(|c| c.display(f));
            let r = [Some(rank(&key)), name.as_deref().map(rank)]
                .into_iter()
                .flatten()
                .flatten()
                .min();
            if let Some(r) = r {
                found.push((
                    r,
                    Hit {
                        label: layer.display_name(f),
                        detail: layer.name.clone(),
                        target: Target::Feature(layer.id, f as u32),
                    },
                ));
                // Enough exact matches: stop looking further.
                if found.iter().filter(|(r, _)| *r == 0).count() >= limit {
                    break;
                }
            }
        }
    }
    found.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.label.len().cmp(&b.1.label.len()))
    });
    found.into_iter().take(limit).map(|(_, h)| h).collect()
}
