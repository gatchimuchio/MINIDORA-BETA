//! Pure required-artifact graph bookkeeping, owned and driven by the Core.
//!
//! The caller binds nodes to the original goal and supplies hashes only after
//! verifying actual outputs/effect receipts. A hash-shaped string alone is not
//! evidence of a physical result. This module invokes no capability, grants no
//! permission and makes no final goal judgment.

use minidora_domain::{Result, failure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GoalNodeKind {
    CoreResult,
    Evidence,
    ComputedValue,
    ContentPlan,
    RenderedArtifact,
    RequiredFile,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalNode {
    pub id: String,
    pub kind: GoalNodeKind,
    pub requires: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Scheduler {
    nodes: Vec<GoalNode>,
    index: BTreeMap<String, usize>,
    observed: BTreeMap<String, String>,
    consumption: BTreeMap<String, BTreeMap<String, String>>,
    remaining_actions: u32,
}

impl Scheduler {
    pub fn new(nodes: Vec<GoalNode>, action_budget: u32) -> Result<Self> {
        if nodes.is_empty() {
            return Err(failure(
                "GOAL_CONTRACT_REQUIRED",
                "A required-artifact graph must contain an explicit goal node",
            ));
        }
        let mut index = BTreeMap::new();
        for (position, node) in nodes.iter().enumerate() {
            if node.id.is_empty() || index.insert(node.id.clone(), position).is_some() {
                return Err(failure(
                    "INVALID_GOAL_NODE",
                    "Required artifact identities must be nonempty and unique",
                ));
            }
        }
        let mut indegrees = vec![0usize; nodes.len()];
        let mut dependents = vec![Vec::new(); nodes.len()];
        for (position, node) in nodes.iter().enumerate() {
            let mut unique = BTreeSet::new();
            for prerequisite in &node.requires {
                let dependency = *index.get(prerequisite).ok_or_else(|| {
                    failure(
                        "MISSING_GOAL_NODE",
                        format!("Node {} requires absent node {prerequisite}", node.id),
                    )
                })?;
                if !unique.insert(prerequisite) {
                    return Err(failure(
                        "DUPLICATE_PREREQUISITE",
                        format!("Node {} repeats prerequisite {prerequisite}", node.id),
                    ));
                }
                indegrees[position] += 1;
                dependents[dependency].push(position);
            }
        }
        let mut frontier: VecDeque<usize> = indegrees
            .iter()
            .enumerate()
            .filter_map(|(position, count)| (*count == 0).then_some(position))
            .collect();
        let mut visited = 0;
        while let Some(position) = frontier.pop_front() {
            visited += 1;
            for dependent in &dependents[position] {
                indegrees[*dependent] -= 1;
                if indegrees[*dependent] == 0 {
                    frontier.push_back(*dependent);
                }
            }
        }
        if visited != nodes.len() {
            return Err(failure(
                "CYCLIC_GOAL_GRAPH",
                "Required-artifact dependencies contain a cycle",
            ));
        }
        Ok(Self {
            nodes,
            index,
            observed: BTreeMap::new(),
            consumption: BTreeMap::new(),
            remaining_actions: action_budget,
        })
    }

    /// All currently ready nodes, in original declaration order. This is not
    /// an ID-based choice between different semantic outcomes. The Core owns
    /// task priorities, authority and any required physical effect ordering.
    pub fn ready(&self) -> Vec<String> {
        if self.remaining_actions == 0 {
            return Vec::new();
        }
        self.nodes
            .iter()
            .filter(|node| {
                !self.observed.contains_key(&node.id)
                    && node
                        .requires
                        .iter()
                        .all(|prerequisite| self.observed.contains_key(prerequisite))
            })
            .map(|node| node.id.clone())
            .collect()
    }

    /// Record one actual Core-verified result. Every attempted observation
    /// consumes the task's finite action budget, including a rejected duplicate
    /// or stale observation. Invalid observations never advance the graph.
    /// The Core must check ready()/remaining_actions() before invoking effects.
    pub fn observe(
        &mut self,
        id: &str,
        artifact_sha: &str,
        consumed: &BTreeMap<String, String>,
    ) -> Result<()> {
        if self.remaining_actions == 0 {
            return Err(failure(
                "ACTION_BUDGET_EXHAUSTED",
                "The finite task action budget is exhausted with residual nodes",
            ));
        }
        self.remaining_actions -= 1;
        let node = self
            .index
            .get(id)
            .map(|position| &self.nodes[*position])
            .ok_or_else(|| failure("MISSING_GOAL_NODE", "Observation has no required goal node"))?;
        if artifact_sha.len() != 64
            || !artifact_sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(failure(
                "INVALID_ARTIFACT_REFERENCE",
                "An observed artifact requires a canonical lowercase SHA-256 reference",
            ));
        }
        if let Some(previous) = self.observed.get(id) {
            return Err(failure(
                if previous == artifact_sha {
                    "NO_PROGRESS"
                } else {
                    "ARTIFACT_CONFLICT"
                },
                "A completed required node cannot be repeated or silently replaced",
            ));
        }
        if consumed.len() != node.requires.len()
            || node
                .requires
                .iter()
                .any(|prerequisite| !consumed.contains_key(prerequisite))
        {
            return Err(failure(
                "PREREQUISITE_SET_MISMATCH",
                "The actual consumed artifact set must exactly match the declared prerequisites",
            ));
        }
        for prerequisite in &node.requires {
            let actual = self.observed.get(prerequisite).ok_or_else(|| {
                failure(
                    "UNOBSERVED_PREREQUISITE",
                    format!("Required predecessor {prerequisite} has no verified output"),
                )
            })?;
            if consumed.get(prerequisite) != Some(actual) {
                return Err(failure(
                    "STALE_PREREQUISITE",
                    format!("Node {id} did not consume the actual output of {prerequisite}"),
                ));
            }
        }
        self.observed.insert(id.into(), artifact_sha.into());
        self.consumption.insert(id.into(), consumed.clone());
        Ok(())
    }

    pub fn residuals(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter(|node| !self.observed.contains_key(&node.id))
            .map(|node| node.id.clone())
            .collect()
    }

    pub fn is_complete(&self) -> bool {
        self.observed.len() == self.nodes.len()
    }

    pub fn remaining_actions(&self) -> u32 {
        self.remaining_actions
    }

    pub fn observed(&self) -> &BTreeMap<String, String> {
        &self.observed
    }

    pub fn consumed_by(&self, node_id: &str) -> Option<&BTreeMap<String, String>> {
        self.consumption.get(node_id)
    }

    pub fn nodes(&self) -> &[GoalNode] {
        &self.nodes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minidora_domain::{Error, digest};

    fn node(id: &str, kind: GoalNodeKind, requires: &[&str]) -> GoalNode {
        GoalNode {
            id: id.into(),
            kind,
            requires: requires.iter().map(|s| (*s).into()).collect(),
        }
    }
    fn pipeline() -> Vec<GoalNode> {
        vec![
            node("compute", GoalNodeKind::ComputedValue, &[]),
            node("content", GoalNodeKind::ContentPlan, &["compute"]),
            node("render", GoalNodeKind::RenderedArtifact, &["content"]),
            node("file", GoalNodeKind::RequiredFile, &["render"]),
        ]
    }
    fn code(error: Error) -> String {
        match error {
            Error::Contract { code, .. } => code,
            error => panic!("unexpected error: {error}"),
        }
    }

    #[test]
    fn downstream_nodes_consume_actual_predecessor_artifacts() {
        let mut scheduler = Scheduler::new(pipeline(), 4).unwrap();
        assert_eq!(scheduler.ready(), vec!["compute"]);
        let mut prior = None;
        for (id, bytes) in [
            ("compute", "2+3=5"),
            ("content", "計算結果は5です。"),
            ("render", "計算結果は5です。\n"),
            ("file", "confirmed exact artifact receipt"),
        ] {
            let consumed = prior.clone().into_iter().collect();
            let artifact = digest(bytes.as_bytes());
            scheduler.observe(id, &artifact, &consumed).unwrap();
            assert_eq!(scheduler.consumed_by(id), Some(&consumed));
            prior = Some((id.to_owned(), artifact));
        }
        assert!(scheduler.is_complete());
        assert!(scheduler.residuals().is_empty());
        assert!(scheduler.ready().is_empty());
        assert_eq!(scheduler.remaining_actions(), 0);
    }

    #[test]
    fn cycles_missing_nodes_and_duplicate_contracts_are_rejected() {
        assert_eq!(
            code(Scheduler::new(vec![], 1).unwrap_err()),
            "GOAL_CONTRACT_REQUIRED"
        );
        assert_eq!(
            code(
                Scheduler::new(vec![node("a", GoalNodeKind::CoreResult, &["missing"])], 1)
                    .unwrap_err()
            ),
            "MISSING_GOAL_NODE"
        );
        assert_eq!(
            code(
                Scheduler::new(
                    vec![
                        node("a", GoalNodeKind::CoreResult, &["b"]),
                        node("b", GoalNodeKind::CoreResult, &["a"])
                    ],
                    2
                )
                .unwrap_err()
            ),
            "CYCLIC_GOAL_GRAPH"
        );
        assert_eq!(
            code(Scheduler::new(vec![node("a", GoalNodeKind::CoreResult, &["a"])], 1).unwrap_err()),
            "CYCLIC_GOAL_GRAPH"
        );
        assert_eq!(
            code(
                Scheduler::new(
                    vec![
                        node("a", GoalNodeKind::CoreResult, &[]),
                        node("a", GoalNodeKind::CoreResult, &[])
                    ],
                    2
                )
                .unwrap_err()
            ),
            "INVALID_GOAL_NODE"
        );
    }

    #[test]
    fn stale_and_unobserved_dependencies_never_advance_the_graph() {
        let mut scheduler = Scheduler::new(pipeline(), 8).unwrap();
        let value = digest(b"actual value");
        let consumed = BTreeMap::from([("compute".into(), value.clone())]);
        assert_eq!(
            code(
                scheduler
                    .observe("content", &digest(b"plan"), &consumed)
                    .unwrap_err()
            ),
            "UNOBSERVED_PREREQUISITE"
        );
        scheduler
            .observe("compute", &value, &BTreeMap::new())
            .unwrap();
        let fake = BTreeMap::from([("compute".into(), digest(b"imagined value"))]);
        assert_eq!(
            code(
                scheduler
                    .observe("content", &digest(b"plan"), &fake)
                    .unwrap_err()
            ),
            "STALE_PREREQUISITE"
        );
        assert_eq!(scheduler.ready(), vec!["content"]);
        assert!(!scheduler.observed().contains_key("content"));
        scheduler
            .observe("content", &digest(b"plan"), &consumed)
            .unwrap();
        assert_eq!(scheduler.ready(), vec!["render"]);
    }

    #[test]
    fn no_progress_and_exhaustion_do_not_manufacture_completion() {
        let mut scheduler = Scheduler::new(pipeline(), 2).unwrap();
        let value = digest(b"actual value");
        scheduler
            .observe("compute", &value, &BTreeMap::new())
            .unwrap();
        assert_eq!(
            code(
                scheduler
                    .observe("compute", &value, &BTreeMap::new())
                    .unwrap_err()
            ),
            "NO_PROGRESS"
        );
        assert_eq!(scheduler.remaining_actions(), 0);
        assert!(scheduler.ready().is_empty());
        assert_eq!(scheduler.residuals(), vec!["content", "render", "file"]);
        assert!(!scheduler.is_complete());
        assert_eq!(
            code(
                scheduler
                    .observe(
                        "content",
                        &digest(b"plan"),
                        &BTreeMap::from([("compute".into(), value)])
                    )
                    .unwrap_err()
            ),
            "ACTION_BUDGET_EXHAUSTED"
        );
    }

    #[test]
    fn exact_prerequisites_and_canonical_references_are_required() {
        let mut scheduler = Scheduler::new(pipeline(), 8).unwrap();
        assert_eq!(
            code(
                scheduler
                    .observe("compute", "fake", &BTreeMap::new())
                    .unwrap_err()
            ),
            "INVALID_ARTIFACT_REFERENCE"
        );
        assert_eq!(
            code(
                scheduler
                    .observe(
                        "compute",
                        &digest(b"actual"),
                        &BTreeMap::from([("unrelated".into(), digest(b"else"))])
                    )
                    .unwrap_err()
            ),
            "PREREQUISITE_SET_MISMATCH"
        );
        scheduler
            .observe("compute", &digest(b"actual"), &BTreeMap::new())
            .unwrap();
        assert_eq!(
            code(
                scheduler
                    .observe("compute", &digest(b"replacement"), &BTreeMap::new())
                    .unwrap_err()
            ),
            "ARTIFACT_CONFLICT"
        );
        assert_eq!(
            scheduler.observed().get("compute"),
            Some(&digest(b"actual"))
        );
    }

    #[test]
    fn ready_list_retains_explicit_goal_order_not_lexical_identity() {
        let scheduler = Scheduler::new(
            vec![
                node("z-first", GoalNodeKind::Evidence, &[]),
                node("a-second", GoalNodeKind::Evidence, &[]),
            ],
            2,
        )
        .unwrap();
        assert_eq!(scheduler.ready(), vec!["z-first", "a-second"]);
    }
}
