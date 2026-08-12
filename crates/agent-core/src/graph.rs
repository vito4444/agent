use crate::error::{CoreError, Result};
use crate::types::TaskGraph;
use std::collections::{HashMap, HashSet};

/// Human-pasted YAML planner input for V0.
/// Scheduler/Gate/Merge must never call an LLM — parsing stays deterministic here.
pub fn parse_graph_yaml(yaml: &str) -> Result<TaskGraph> {
    let graph: TaskGraph = serde_yaml::from_str(yaml)?;
    validate_graph(&graph)?;
    Ok(graph)
}

pub fn validate_graph(graph: &TaskGraph) -> Result<()> {
    if graph.tasks.is_empty() {
        return Err(CoreError::InvalidGraph("no tasks".into()));
    }
    let ids: HashSet<_> = graph.tasks.iter().map(|t| t.id.as_str()).collect();
    if ids.len() != graph.tasks.len() {
        return Err(CoreError::InvalidGraph("duplicate task id".into()));
    }
    for dep in &graph.deps {
        if !ids.contains(dep.from.as_str()) {
            return Err(CoreError::InvalidGraph(format!(
                "dep.from unknown: {}",
                dep.from
            )));
        }
        if !ids.contains(dep.to.as_str()) {
            return Err(CoreError::InvalidGraph(format!(
                "dep.to unknown: {}",
                dep.to
            )));
        }
        if dep.from == dep.to {
            return Err(CoreError::InvalidGraph("self-dep".into()));
        }
    }
    detect_cycle(graph)?;
    Ok(())
}

fn detect_cycle(graph: &TaskGraph) -> Result<()> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for t in &graph.tasks {
        adj.entry(t.id.as_str()).or_default();
    }
    for d in &graph.deps {
        adj.entry(d.from.as_str())
            .or_default()
            .push(d.to.as_str());
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    fn dfs<'a>(
        n: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        visiting: &mut HashSet<&'a str>,
        visited: &mut HashSet<&'a str>,
    ) -> Result<()> {
        if visited.contains(n) {
            return Ok(());
        }
        if !visiting.insert(n) {
            return Err(CoreError::InvalidGraph(format!("cycle at {n}")));
        }
        if let Some(next) = adj.get(n) {
            for m in next {
                dfs(m, adj, visiting, visited)?;
            }
        }
        visiting.remove(n);
        visited.insert(n);
        Ok(())
    }
    for t in &graph.tasks {
        dfs(t.id.as_str(), &adj, &mut visiting, &mut visited)?;
    }
    Ok(())
}

pub fn demo_two_task_yaml() -> &'static str {
    // Kept in sync with fixtures/demo-project/graph.yaml (source of truth for demos).
    r#"
name: fix-and-test
tasks:
  - id: A
    title: Fix add function
    prompt: |
      Fix src/lib.rs so add(2,2) returns 4 (use a+b).
      Produce the fixed file as artifact fixed_lib.
    gate_command: "cargo test --manifest-path Cargo.toml add_works"
    produces:
      - id: fixed_lib
        path: src/lib.rs
        description: Fixed library source
  - id: B
    title: Write more tests
    prompt: |
      Add tests/extra.rs covering add(0,0) and add(-1,1).
      Upstream fixed_lib must already be present in this worktree.
    gate_command: "test -f tests/extra.rs && cargo test --manifest-path Cargo.toml"
    produces:
      - id: extra_tests
        path: tests/extra.rs
deps:
  - from: A
    to: B
    artifacts:
      - id: fixed_lib
        path: src/lib.rs
"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_demo_yaml() {
        let g = parse_graph_yaml(demo_two_task_yaml()).unwrap();
        assert_eq!(g.tasks.len(), 2);
        assert_eq!(g.deps.len(), 1);
        assert_eq!(g.deps[0].artifacts[0].id, "fixed_lib");
    }

    #[test]
    fn rejects_cycle() {
        let yaml = r#"
name: c
tasks:
  - id: A
    title: a
    prompt: p
  - id: B
    title: b
    prompt: p
deps:
  - from: A
    to: B
    artifacts: []
  - from: B
    to: A
    artifacts: []
"#;
        assert!(parse_graph_yaml(yaml).is_err());
    }
}
