//! Adapter from cua-driver `get_window_state` output to the axdiff tree model.
//!
//! cua returns a FLAT `elements` array in depth-first order; each element carries
//! `element_index`, `parent_index`, `depth`, `role`, optional `label`, optional
//! `value`, `frame`, `element_token`, `actions`. We reconstruct the nested tree
//! by `parent_index` (an element is a root when its `parent_index` is absent,
//! negative, equal to its own index, or points outside the set), preserving the
//! array's sibling order.
//!
//! Real-data note (validated against live Activity Monitor, 2026-09-10): cua's
//! `parent_index` and `depth` can disagree. Many AXRow elements carry
//! `parent_index = 0` (the AXWindow) yet `depth = 2`, because cua flattens the
//! intermediate container (AXTable/AXOutline/AXScrollArea) in `parent_index`
//! while `depth` preserves the true AX nesting. We treat `parent_index` as the
//! authoritative tree structure (it is an explicit parent pointer and groups
//! siblings consistently); `depth` is a display hint that can differ. A
//! depth-based reconstruction is possible but would need to synthesize the
//! omitted container nodes, so it is not the default.
use crate::RawNode;
use serde_json::Value;
use std::collections::HashMap;

/// Parse a cua `get_window_state` JSON document into an axdiff forest.
/// Accepts a top-level or structuredContent snapshot. Passive observations have
/// no native input identity, even if a malformed producer supplies one.
pub fn forest_from_gws(doc: &Value) -> Vec<RawNode> {
    let doc = if doc.get("elements").is_some() { doc } else { doc.get("structuredContent").unwrap_or(doc) };
    let mut rows: Vec<_> = doc.get("elements").and_then(Value::as_array)
        .into_iter().flatten().filter_map(indexed_row).collect();
    for e in doc.get("passive_elements").and_then(Value::as_array).into_iter().flatten() {
        let mut node = observation_node(e);
        node.source = None;
        rows.push((e.get("parent_index").and_then(Value::as_i64), node));
    }
    nest_flat_nodes(rows)
}

/// Parse a flat cua `elements` array into an axdiff forest.
pub fn forest_from_elements(elements: &[Value]) -> Vec<RawNode> {
    nest_flat_nodes(elements.iter().filter_map(indexed_row).collect())
}

fn indexed_row(e: &Value) -> Option<(Option<i64>, RawNode)> {
    let index = e.get("element_index").and_then(Value::as_i64)?;
    let mut node = observation_node(e);
    node.source = Some(index);
    Some((e.get("parent_index").and_then(Value::as_i64), node))
}

fn observation_node(e: &Value) -> RawNode {
    let label = ["label", "name", "title"].iter()
        .find_map(|key| e.get(*key).and_then(Value::as_str))
        .unwrap_or("").to_string();
    let value = e.get("value").and_then(|value| match value {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        other => Some(other.to_string()),
    });
    RawNode {
        role: e.get("role").and_then(Value::as_str).unwrap_or("").into(),
        label, value, selected: e.get("selected").and_then(Value::as_bool).unwrap_or(false),
        source: None,
        identity: e.get("identifier").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string),
        frame: None,
        children: Vec::new(),
    }
}

/// Levels a nested forest may have. Rows deeper than the last level are placed on
/// it, in walk order, under their ancestor one level up. Every tree walk here is
/// recursive, and a saved lineage must load again: serde_json refuses nesting past
/// 128, which a tree of 64 levels reaches.
pub const MAX_DEPTH: usize = 60;

/// Nest observations by their nearest indexed parent. Shared with the native
/// driver projections so passive rows never gain an invented source index. Each row
/// is placed once, so duplicate indexes cannot repeat a subtree or loop.
pub fn nest_flat_nodes(rows: Vec<(Option<i64>, RawNode)>) -> Vec<RawNode> {
    let present: std::collections::HashSet<i64> =
        rows.iter().filter_map(|(_, node)| node.source).collect();
    let mut children: HashMap<i64, Vec<usize>> = HashMap::new();
    let mut roots: Vec<usize> = Vec::new();
    for (pos, (parent, node)) in rows.iter().enumerate() {
        match parent {
            Some(p) if *p >= 0 && Some(*p) != node.source && present.contains(p) => children.entry(*p).or_default().push(pos),
            _ => roots.push(pos),
        }
    }
    struct Ctx<'a> { rows: &'a [(Option<i64>, RawNode)], children: HashMap<i64, Vec<usize>>, placed: Vec<bool> }
    impl Ctx<'_> {
        fn kids(&self, pos: usize) -> Vec<usize> {
            self.rows[pos].1.source.and_then(|index| self.children.get(&index)).cloned().unwrap_or_default()
        }
        fn build(&mut self, pos: usize, depth: usize) -> RawNode {
            let mut node = self.rows[pos].1.clone();
            let kids: Vec<usize> = self.kids(pos).into_iter().filter(|&c| !std::mem::replace(&mut self.placed[c], true)).collect();
            if depth + 2 < MAX_DEPTH {
                node.children = kids.into_iter().map(|c| self.build(c, depth + 1)).collect();
                return node;
            }
            // The last level: the whole subtree below, depth first, as leaves.
            let mut stack: Vec<usize> = kids.into_iter().rev().collect();
            while let Some(c) = stack.pop() {
                let mut leaf = self.rows[c].1.clone();
                leaf.children = Vec::new();
                node.children.push(leaf);
                let below: Vec<usize> = self.kids(c).into_iter().filter(|&k| !std::mem::replace(&mut self.placed[k], true)).collect();
                stack.extend(below.into_iter().rev());
            }
            node
        }
    }
    let mut ctx = Ctx { rows: &rows, children, placed: vec![false; rows.len()] };
    for &r in &roots { ctx.placed[r] = true; }
    roots.iter().map(|&r| ctx.build(r, 0)).collect()
}
