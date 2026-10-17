//! Codex/Sky `UIElementRenderDifference`-parity AX-tree diff engine.
//!
//! cua-driver returns a full accessibility tree every turn and invalidates the
//! prior index map on each snapshot (see specs/02-ax-diffing.md section 3). Codex
//! instead carries a `UIElementTreeRevision` lineage: each turn's tree inherits
//! stable element IDs from the previous turn by tree path, and `get_app_state`
//! returns only the changed/added/removed rows (`~`/`+`/`-`), with removed rows
//! optionally collapsed into element-ID ranges, and a line-budget guard that
//! falls back to a full render when the change set is too large.
//!
//! This crate recreates that subsystem over a structured element tree (the
//! "recommended first cut" from spec 02 step 64): stable cross-snapshot identity
//! plus the diff renderer, with the exact header/legend strings the model must
//! see (conformance/golden/ax-diff-format.md).
//!
//! Grounding:
//! - Header/legend/no-change strings: verbatim from golden/ax-diff-format.md.
//! - Monotonic element ID (+1 per new element, inherited by matched nodes):
//!   decompile finding `NextAvailableElementIDIterator.next()` (specs/20).
//! - Structural pairing old-vs-new by tree path in depth-first order, unchanged
//!   nodes inherit their prior ID: `UIElementRenderDifference.Change{path,
//!   depthFirstOrder}` + `inheritElementID()` (specs/02 section 2).
//!
//! INFERRED (not recoverable from the stripped Swift): the exact line-budget
//! constant and the precise per-row text beyond the marker. The budget is a
//! parameter here (default `DEFAULT_LINE_BUDGET`); the golden pins only the
//! markers, headers, and legends, which this crate emits verbatim.

pub mod cua;
pub mod layer;
pub mod sky;
pub mod recovery;

use std::collections::BTreeSet;
use std::fmt::Write as _;

/// Default diff line budget. INFERRED: Codex's value lives in a stripped Swift
/// constant; `differenceLines` returns nil past it and Sky re-renders full. We
/// expose it as a parameter and pick a conservative default.
pub const DEFAULT_LINE_BUDGET: usize = 200;

// Exact strings the model must see. Verbatim from conformance/golden/ax-diff-format.md.
pub const HDR_INCREMENTAL: &str = "The following is a diff from the previous accessibility tree";
pub const LEGEND_INCREMENTAL: &str =
    "with ~, +, and - representing changed, added, and removed elements, respectively.";
pub const HDR_CUMULATIVE: &str = "The following is a cumulative diff from the initial accessibility tree";
pub const LEGEND_CUMULATIVE: &str = "with ~ and + representing changed and added elements, respectively. Removed elements are summarized by ID range.";
pub const NO_CHANGE_PREFIX: &str = "There has been no change in the accessibility tree for ";

/// A raw accessibility node as walked from the platform AX tree, before identity
/// assignment. `label` is the AX name/title (may be empty); `value` is the
/// settable/displayed value when present.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawNode {
    pub role: String,
    pub label: String,
    pub value: Option<String>,
    /// Platform "selected" state (cua `selected`); carried, not diffed.
    pub selected: bool,
    /// The platform's per-snapshot index for this node (cua `element_index`),
    /// so a stable id can be mapped back to the index the driver acts on.
    pub source: Option<i64>,
    /// Platform identifier, scoped to a matched parent. Never a snapshot token/index.
    pub identity: Option<String>,
    /// Window-relative bounds `[x, y, w, h]` in whole points, when the platform
    /// reports them. Evidence for an otherwise ambiguous unchanged sibling only;
    /// never part of change classification.
    pub frame: Option<[i64; 4]>,
    pub children: Vec<RawNode>,
}

impl RawNode {
    pub fn new(role: &str, label: &str) -> Self {
        RawNode { role: role.into(), label: label.into(), value: None, selected: false, source: None, identity: None, frame: None, children: Vec::new() }
    }
    pub fn with_value(mut self, v: &str) -> Self {
        self.value = Some(v.into());
        self
    }
    pub fn at(mut self, frame: [i64; 4]) -> Self {
        self.frame = Some(frame);
        self
    }
    pub fn kids(mut self, children: Vec<RawNode>) -> Self {
        self.children = children;
        self
    }
}

/// A node after identity assignment: carries a stable `id` inherited across turns.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Element {
    pub id: u64,
    pub role: String,
    pub label: String,
    pub value: Option<String>,
    pub depth: usize,
    /// Platform "selected" state at this snapshot (window2 `selected_elements`).
    /// Not part of identity or change classification. `default` so persisted
    /// state written before this field existed still loads.
    #[serde(default)]
    pub selected: bool,
    /// The platform index this element had in the snapshot it came from.
    #[serde(default)]
    pub source: Option<i64>,
    #[serde(default)]
    pub identity: Option<String>,
    #[serde(default)]
    pub frame: Option<[i64; 4]>,
    pub children: Vec<Element>,
}

/// How a node in the NEW tree relates to the PREVIOUS revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Change {
    Added,
    Changed,
    Unchanged,
}

/// One assigned tree plus its monotonic id counter. This is the analogue of
/// Codex's `UIElementTreeRevision`: to advance a lineage you call
/// [`Revision::next`] with the next raw tree, and matched nodes keep their id.
#[derive(Clone, Debug)]
pub struct Revision {
    pub root: Vec<Element>,
    next_id: u64,
    // Parallel classification of each Element (by id) for diff rendering.
    changes: std::collections::HashMap<u64, Change>,
    // Ids present in the PREVIOUS revision that are gone from this one.
    removed_ids: Vec<u64>,
}

impl Revision {
    /// The initial revision for a lineage (first snapshot). Every node is newly
    /// identified starting at id 1; nothing is a "change" yet.
    pub fn initial(raw: &[RawNode]) -> Revision {
        let mut next_id = 1u64;
        let mut changes = std::collections::HashMap::new();
        let root = assign_new_forest(raw, 0, &mut next_id, &mut changes);
        Revision { root, next_id, changes, removed_ids: Vec::new() }
    }

    /// Advance the lineage: produce the next revision from this one and the next
    /// raw tree. Matched nodes inherit their id (unchanged or `Changed` if the
    /// value differs); unmatched new nodes draw the next id (`Added`); prior
    /// nodes with no match are recorded in `removed_ids`.
    pub fn next(&self, raw: &[RawNode]) -> Revision {
        let mut next_id = self.next_id;
        let mut changes = std::collections::HashMap::new();
        let mut removed_ids = Vec::new();
        let root = match_forest(&self.root, raw, 0, &mut next_id, &mut changes, &mut removed_ids);
        removed_ids.sort_unstable();
        Revision { root, next_id, changes, removed_ids }
    }

    /// Reconstruct a revision from persisted identity state (`root` forest and
    /// the `next_id` counter). The transient diff classification is empty; use
    /// this as a baseline to call [`Revision::next`] against a fresh tree.
    pub fn restore(root: Vec<Element>, next_id: u64) -> Revision {
        Revision { root, next_id, changes: std::collections::HashMap::new(), removed_ids: Vec::new() }
    }

    /// The persisted identity state: the assigned forest and the id counter.
    pub fn state(&self) -> (&[Element], u64) {
        (&self.root, self.next_id)
    }

    /// True when this revision differs from the one it was stepped from
    /// (any added/changed node or any removed id). False for an initial revision.
    pub fn any_change(&self) -> bool {
        !self.removed_ids.is_empty()
            || self.changes.values().any(|c| matches!(c, Change::Added | Change::Changed))
    }

    /// Flatten to `id -> (role, label, value)` over the whole forest. Used as a
    /// baseline for [`render_cumulative`].
    fn flat_by_id(&self) -> std::collections::HashMap<u64, (String, String, Option<String>)> {
        let mut m = std::collections::HashMap::new();
        fn go(es: &[Element], m: &mut std::collections::HashMap<u64, (String, String, Option<String>)>) {
            for e in es {
                m.insert(e.id, (e.role.clone(), e.label.clone(), e.value.clone()));
                go(&e.children, m);
            }
        }
        go(&self.root, &mut m);
        m
    }

    /// The set of element ids present in this revision.
    fn id_set(&self) -> std::collections::HashSet<u64> {
        let mut s = std::collections::HashSet::new();
        fn go(es: &[Element], s: &mut std::collections::HashSet<u64>) {
            for e in es {
                s.insert(e.id);
                go(&e.children, s);
            }
        }
        go(&self.root, &mut s);
        s
    }
}

// Assign fresh ids to a whole forest (used for the initial revision and for
// added subtrees). Everything gets classified `Added` when `classify_added` is
// true (subtree under a new parent), otherwise left unclassified (initial).
fn assign_new_forest(
    raw: &[RawNode],
    depth: usize,
    next_id: &mut u64,
    changes: &mut std::collections::HashMap<u64, Change>,
) -> Vec<Element> {
    raw.iter()
        .map(|r| {
            let id = *next_id;
            *next_id += 1; // monotonic +1, per NextAvailableElementIDIterator (specs/20)
            let children = assign_new_forest(&r.children, depth + 1, next_id, changes);
            Element { id, role: r.role.clone(), label: r.label.clone(), value: r.value.clone(), depth, selected: r.selected, source: r.source, identity: r.identity.clone(), frame: r.frame, children }
        })
        .collect()
}

// Same as assign_new_forest but marks every produced node `Added`.
fn assign_added_forest(
    raw: &[RawNode],
    depth: usize,
    next_id: &mut u64,
    changes: &mut std::collections::HashMap<u64, Change>,
) -> Vec<Element> {
    raw.iter()
        .map(|r| {
            let id = *next_id;
            *next_id += 1;
            changes.insert(id, Change::Added);
            let children = assign_added_forest(&r.children, depth + 1, next_id, changes);
            Element { id, role: r.role.clone(), label: r.label.clone(), value: r.value.clone(), depth, selected: r.selected, source: r.source, identity: r.identity.clone(), frame: r.frame, children }
        })
        .collect()
}

// Content anchoring is an inference, not platform object equality. Known conflicting
// identifiers always veto a match. Anonymous containers need a shared descendant
// anchor; equal roles/positions alone say nothing about a replaced subtree.
fn identity_matches(o: &Element, r: &RawNode) -> bool {
    o.role == r.role && o.identity.is_some() && o.identity == r.identity
}
fn content_matches(o: &Element, r: &RawNode) -> bool {
    if o.role != r.role || o.label != r.label { return false; }
    if o.identity.is_some() && r.identity.is_some() && o.identity != r.identity { return false; }
    if identity_matches(o, r) { return true; }
    if o.children.is_empty() && r.children.is_empty() { return true; }
    // Sky rows key on `role/subrole` (`AXWindow/AXStandardWindow`)
    if matches!(o.role.split('/').next().unwrap_or(""), "AXWindow" | "Window") { return true; }
    o.children.iter().any(|oc| r.children.iter().any(|rc| identity_matches(oc, rc) || content_matches(oc, rc)))
}
fn same_subtree(o: &Element, r: &RawNode) -> bool {
    content_matches(o, r) && o.children.len() == r.children.len()
        && o.children.iter().zip(&r.children).all(|(oc, rc)| same_subtree(oc, rc))
}

/// Nothing observable about this node differs: content, subtree and
/// window-relative frame. Used only to pair members of an ambiguous group.
fn unchanged_in_place(o: &Element, r: &RawNode) -> bool {
    o.frame.is_some() && o.frame == r.frame && unchanged_subtree(o, r)
}

fn unchanged_subtree(o: &Element, r: &RawNode) -> bool {
    o.role == r.role && o.label == r.label && o.value == r.value && o.identity == r.identity
        && o.children.len() == r.children.len()
        && o.children.iter().zip(&r.children).all(|(oc, rc)| unchanged_subtree(oc, rc))
}

// Match a NEW sibling list against the OLD assigned siblings. Greedy, order-
// preserving match by (role,label); first unconsumed old sibling with the same
// key wins, mirroring depth-first pairing.
fn match_forest(
    old: &[Element],
    new: &[RawNode],
    depth: usize,
    next_id: &mut u64,
    changes: &mut std::collections::HashMap<u64, Change>,
    removed_ids: &mut Vec<u64>,
) -> Vec<Element> {
    let mut consumed = vec![false; old.len()];
    // matched_old[new_index] = the old sibling index this new node inherits from.
    let mut matched_old: Vec<Option<usize>> = vec![None; new.len()];

    // An identical sibling forest has no observable replacement or reorder.
    // Keep its positional IDs, including duplicate labels; once content changes
    // the evidence-based passes below must establish each match independently.
    if old.len() == new.len() && old.iter().zip(new).all(|(o, r)| unchanged_subtree(o, r)) {
        for i in 0..new.len() { matched_old[i] = Some(i); consumed[i] = true; }
    }

    // Each pass accepts only a unique pair in BOTH original sibling lists. Do
    // not let consuming B make the last unrelated A/C pair look unambiguous.
    // 1. Unique platform identifier (relabels and moves supported).
    // 2. Whole-subtree content anchors (disambiguates anonymous sibling groups).
    // 3. Unique content with a compatible subtree (value/child edits supported).
    // No same-role fallback: even one leftover node at the same position may
    // be a removal/addition. Snapshot tokens change on every observation.
    for matches in [identity_matches, same_subtree, content_matches] {
        for (ni, r) in new.iter().enumerate() {
            if matched_old[ni].is_some() { continue; }
            let candidates: Vec<usize> = old.iter().enumerate()
                .filter(|(_, o)| matches(o, r)).map(|(i, _)| i).collect();
            if candidates.len() != 1 { continue; }
            let oi = candidates[0];
            if consumed[oi] || new.iter().filter(|n| matches(&old[oi], n)).count() != 1 { continue; }
            // Duplicate platform identifiers cannot establish a relabel, even
            // if content matching could otherwise narrow the leftovers.
            if old[oi].label != r.label && (
                old.iter().filter(|o| identity_matches(o, r)).count() != 1 ||
                new.iter().filter(|n| identity_matches(&old[oi], n)).count() != 1
            ) { continue; }
            consumed[oi] = true;
            matched_old[ni] = Some(oi);
        }
    }

    // 4. Unchanged in place. Identical siblings without a platform identifier
    // (AppKit's unlabelled title-bar buttons) are ambiguous to every pass above,
    // so an unrelated change elsewhere used to re-mint them. When the leftover
    // group has the same size before and after, a member whose content, subtree
    // and window-relative frame are all unchanged keeps its id, provided exactly
    // one old member and one new member share that frame. A removal (group
    // shrinks), a moved control or any content change still re-mints: position
    // alone, or position with a changed value, is not identity. The pairing is an
    // inference from unchanged observations, not native object equality.
    let (unmatched_new, unconsumed_old) = (matched_old.clone(), consumed.clone());
    let group_sizes_equal = |r: &RawNode| {
        new.iter().enumerate().filter(|(i, n)| unmatched_new[*i].is_none() && n.role == r.role && n.label == r.label).count()
            == old.iter().enumerate().filter(|(i, o)| !unconsumed_old[*i] && o.role == r.role && o.label == r.label).count()
    };
    for ni in 0..new.len() {
        if matched_old[ni].is_some() { continue; }
        let r = &new[ni];
        if !group_sizes_equal(r) { continue; }
        let candidates: Vec<usize> = old.iter().enumerate()
            .filter(|(i, o)| !consumed[*i] && unchanged_in_place(o, r)).map(|(i, _)| i).collect();
        if candidates.len() != 1 { continue; }
        let oi = candidates[0];
        let rivals = new.iter().enumerate()
            .filter(|(i, n)| matched_old[*i].is_none() && unchanged_in_place(&old[oi], n)).count();
        if rivals != 1 { continue; }
        consumed[oi] = true;
        matched_old[ni] = Some(oi);
    }

    // Build the new sibling list in new order from the pairing above.
    let mut out: Vec<Element> = Vec::with_capacity(new.len());
    for (ni, r) in new.iter().enumerate() {
        match matched_old[ni] {
            Some(i) => {
                let o = &old[i];
                let id = o.id; // inheritElementID()
                // A label change is a real change (shows as `~`), same as a value
                // change; a pure positional re-pair with identical content is
                // Unchanged.
                let changed = o.value != r.value || o.label != r.label;
                if changed {
                    changes.insert(id, Change::Changed);
                } else {
                    changes.insert(id, Change::Unchanged);
                }
                let children =
                    match_forest(&o.children, &r.children, depth + 1, next_id, changes, removed_ids);
                out.push(Element {
                    id,
                    role: r.role.clone(),
                    label: r.label.clone(),
                    value: r.value.clone(),
                    depth,
                    selected: r.selected,
                    source: r.source,
                    identity: r.identity.clone(),
                    frame: r.frame,
                    children,
                });
            }
            None => {
                // new node -> Added, plus its whole subtree
                let id = *next_id;
                *next_id += 1;
                changes.insert(id, Change::Added);
                let children = assign_added_forest(&r.children, depth + 1, next_id, changes);
                out.push(Element {
                    id,
                    role: r.role.clone(),
                    label: r.label.clone(),
                    value: r.value.clone(),
                    depth,
                    selected: r.selected,
                    source: r.source,
                    identity: r.identity.clone(),
                    frame: r.frame,
                    children,
                });
            }
        }
    }
    // any old sibling never consumed -> removed (record its whole subtree's ids)
    for (i, o) in old.iter().enumerate() {
        if !consumed[i] {
            collect_ids(o, removed_ids);
        }
    }
    out
}

fn collect_ids(e: &Element, out: &mut Vec<u64>) {
    out.push(e.id);
    for c in &e.children {
        collect_ids(c, out);
    }
}

/// Options for [`render_diff`] / [`render_cumulative`].
///
/// The diff BASELINE is chosen by which function you call, not by a flag:
/// [`render_diff`] diffs against the PREVIOUS turn (incremental header, removed
/// shown as `-` lines); [`render_cumulative`] diffs against the INITIAL turn
/// (cumulative header, removed summarized as ID ranges). This mirrors Codex's
/// two rendered variants (golden/ax-diff-format.md) and avoids the earlier bug
/// where a flag swapped the header but not the baseline.
#[derive(Clone, Debug)]
pub struct DiffOptions {
    /// Per-call opt-out. When true, always return the full tree render.
    pub disable_diff: bool,
    /// Line budget; if the diff exceeds it, the renderer returns
    /// [`DiffOutput::FullFallback`] so the caller renders the full tree.
    pub line_budget: usize,
    /// App name used in the no-change short-circuit line.
    pub app_name: String,
}

impl Default for DiffOptions {
    fn default() -> Self {
        DiffOptions { disable_diff: false, line_budget: DEFAULT_LINE_BUDGET, app_name: String::new() }
    }
}

/// Result of a diff render.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffOutput {
    /// The full tree (first snapshot, `disable_diff`, or budget-exceeded fallback).
    Full(String),
    /// A diff render (`~`/`+`/`-` markers with headers/legend).
    Diff(String),
    /// The no-change short-circuit line.
    NoChange(String),
    /// The diff exceeded the line budget; the caller should render the full tree.
    /// (Carried alongside the full render for convenience.)
    FullFallback(String),
}

impl DiffOutput {
    pub fn text(&self) -> &str {
        match self {
            DiffOutput::Full(s)
            | DiffOutput::Diff(s)
            | DiffOutput::NoChange(s)
            | DiffOutput::FullFallback(s) => s,
        }
    }
}

/// Render the full tree (no markers), depth-first.
pub fn render_full(rev: &Revision) -> String {
    let mut out = String::new();
    for e in &rev.root {
        render_full_node(e, &mut out);
    }
    out.trim_end().to_string()
}

fn render_full_node(e: &Element, out: &mut String) {
    let _ = writeln!(out, "{}", render_line(e));
    for c in &e.children {
        render_full_node(c, out);
    }
}

/// The formatted line for one element exactly as it appears in the full render
/// (indent by depth, `[id] Role "label" = "value"`).
pub fn render_line(e: &Element) -> String {
    format!("{}[{}] {}{}", "  ".repeat(e.depth), e.id, role_label(e), value_suffix(e))
}

impl Revision {
    /// `source index -> stable id` for every element carrying a source index.
    pub fn index_map(&self) -> std::collections::HashMap<i64, u64> {
        let mut out = std::collections::HashMap::new();
        fn go(es: &[Element], out: &mut std::collections::HashMap<i64, u64>) {
            for e in es {
                if let Some(s) = e.source { out.insert(s, e.id); }
                go(&e.children, out);
            }
        }
        go(&self.root, &mut out);
        out
    }

    /// Formatted lines (as in the full render) of every element whose platform
    /// `selected` flag is set, in depth-first order — window2's `selected_elements`.
    pub fn selected_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        fn go(es: &[Element], out: &mut Vec<String>) {
            for e in es {
                if e.selected { out.push(render_line(e)); }
                go(&e.children, out);
            }
        }
        go(&self.root, &mut out);
        out
    }
}

fn role_label(e: &Element) -> String {
    if e.label.is_empty() {
        e.role.clone()
    } else {
        format!("{} \"{}\"", e.role, e.label)
    }
}
fn value_suffix(e: &Element) -> String {
    match &e.value {
        Some(v) => format!(" = \"{}\"", v),
        None => String::new(),
    }
}

/// Render the diff of `rev` (produced via [`Revision::next`]) against its prior
/// revision. `prior_exists` must be false only for the initial revision.
pub fn render_diff(rev: &Revision, prior_exists: bool, opts: &DiffOptions) -> DiffOutput {
    if opts.disable_diff || !prior_exists {
        return DiffOutput::Full(render_full(rev));
    }
    if !rev.any_change() {
        return DiffOutput::NoChange(format!("{}{}", NO_CHANGE_PREFIX, opts.app_name));
    }

    // Collect diff lines in depth-first order over the NEW tree.
    let mut body_lines: Vec<String> = Vec::new();
    for e in &rev.root {
        diff_lines_node(e, &rev.changes, &mut body_lines);
    }
    // Incremental removals: `-` lines (vs the previous turn).
    let removed_block: Option<String> = if rev.removed_ids.is_empty() {
        None
    } else {
        Some(rev.removed_ids.iter().map(|id| format!("- [{}]", id)).collect::<Vec<_>>().join("\n"))
    };
    let removed_line_count = rev.removed_ids.len();

    let total = body_lines.len() + removed_line_count;
    if total > opts.line_budget {
        // differenceLines(from:) returned nil -> full re-render.
        return DiffOutput::FullFallback(render_full(rev));
    }

    let mut out = String::new();
    let _ = writeln!(out, "{}", HDR_INCREMENTAL);
    let _ = writeln!(out, "{}", LEGEND_INCREMENTAL);
    for l in &body_lines {
        let _ = writeln!(out, "{}", l);
    }
    if let Some(rb) = removed_block {
        let _ = writeln!(out, "{}", rb);
    }
    DiffOutput::Diff(out.trim_end().to_string())
}

/// Render a CUMULATIVE diff of `current` against the `initial` revision of the
/// lineage (Codex's "cumulative diff from the initial accessibility tree").
///
/// Unlike [`render_diff`] (which compares to the immediately previous turn),
/// this compares `current` to `initial` directly, using the stable element ids
/// the lineage carries: an id present in `initial` and `current` whose value
/// differs is `~`; an id in `current` but not `initial` is `+`; an id in
/// `initial` but gone from `current` is removed and summarized as an ID range.
/// `current` MUST descend from `initial` via [`Revision::next`] so their ids
/// share a lineage.
pub fn render_cumulative(current: &Revision, initial: &Revision, opts: &DiffOptions) -> DiffOutput {
    if opts.disable_diff {
        return DiffOutput::Full(render_full(current));
    }
    let base = initial.flat_by_id();
    let cur_ids = current.id_set();

    // Added/Changed lines in current depth-first order.
    let mut body_lines: Vec<String> = Vec::new();
    for e in &current.root {
        cumulative_lines_node(e, &base, &mut body_lines);
    }
    // Removed = ids in initial not present in current.
    let mut removed: Vec<u64> = base.keys().copied().filter(|id| !cur_ids.contains(id)).collect();
    removed.sort_unstable();

    if body_lines.is_empty() && removed.is_empty() {
        return DiffOutput::NoChange(format!("{}{}", NO_CHANGE_PREFIX, opts.app_name));
    }

    let removed_line_count = if removed.is_empty() { 0 } else { 1 };
    if body_lines.len() + removed_line_count > opts.line_budget {
        return DiffOutput::FullFallback(render_full(current));
    }

    let mut out = String::new();
    let _ = writeln!(out, "{}", HDR_CUMULATIVE);
    let _ = writeln!(out, "{}", LEGEND_CUMULATIVE);
    for l in &body_lines {
        let _ = writeln!(out, "{}", l);
    }
    if !removed.is_empty() {
        let _ = writeln!(out, "Removed elements: {}", compact_ranges(&removed));
    }
    DiffOutput::Diff(out.trim_end().to_string())
}

fn cumulative_lines_node(
    e: &Element,
    base: &std::collections::HashMap<u64, (String, String, Option<String>)>,
    out: &mut Vec<String>,
) {
    match base.get(&e.id) {
        None => out.push(marker_line('+', e)),                 // not in initial -> added
        Some((role, label, v)) if *role != e.role || *label != e.label || *v != e.value => out.push(marker_line('~', e)), // value changed
        Some(_) => {}                                          // unchanged since initial
    }
    for c in &e.children {
        cumulative_lines_node(c, base, out);
    }
}

fn diff_lines_node(
    e: &Element,
    changes: &std::collections::HashMap<u64, Change>,
    out: &mut Vec<String>,
) {
    match changes.get(&e.id) {
        Some(Change::Added) => out.push(marker_line('+', e)),
        Some(Change::Changed) => out.push(marker_line('~', e)),
        _ => {}
    }
    for c in &e.children {
        diff_lines_node(c, changes, out);
    }
}

fn marker_line(marker: char, e: &Element) -> String {
    let indent = "  ".repeat(e.depth);
    format!("{}{} [{}] {}{}", indent, marker, e.id, role_label(e), value_suffix(e))
}

/// Consolidate a sorted id list into `a-b, c, d-e` ranges.
pub fn compact_ranges(ids: &[u64]) -> String {
    let set: BTreeSet<u64> = ids.iter().copied().collect();
    let mut parts: Vec<String> = Vec::new();
    let mut it = set.into_iter();
    let mut start = match it.next() {
        Some(v) => v,
        None => return String::new(),
    };
    let mut prev = start;
    for v in it {
        if v == prev + 1 {
            prev = v;
        } else {
            parts.push(fmt_range(start, prev));
            start = v;
            prev = v;
        }
    }
    parts.push(fmt_range(start, prev));
    parts.join(", ")
}

fn fmt_range(a: u64, b: u64) -> String {
    if a == b {
        format!("{}", a)
    } else {
        format!("{}-{}", a, b)
    }
}

// ------------------------------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_map_follows_the_new_snapshot_indices_while_ids_stay_stable() {
        let mut a = n("AXRow", "a"); a.source = Some(0);
        let mut b = n("AXRow", "b"); b.source = Some(1);
        let r0 = Revision::initial(&[n("AXWindow", "W").kids(vec![a, b])]);
        assert_eq!(r0.index_map(), [(0, 2), (1, 3)].into_iter().collect());
        // next turn: a new row is inserted first, so cua renumbers a=1, b=2; stable ids stay 2 and 3
        let mut z = n("AXRow", "z"); z.source = Some(0);
        let mut a2 = n("AXRow", "a"); a2.source = Some(1);
        let mut b2 = n("AXRow", "b"); b2.source = Some(2);
        let r1 = r0.next(&[n("AXWindow", "W").kids(vec![z, a2, b2])]);
        assert_eq!(r1.index_map(), [(0, 4), (1, 2), (2, 3)].into_iter().collect());
        let old = r#"{"id":1,"role":"AXWindow","label":"W","value":null,"depth":0,"children":[]}"#;
        assert_eq!(serde_json::from_str::<Element>(old).unwrap().source, None);
    }

    #[test]
    fn selected_lines_are_the_full_render_lines_in_dfs_order_and_survive_matching() {
        let mut a = n("AXRow", "a"); a.selected = true;
        let mut c = n("AXCell", "c"); c.selected = true; c.value = Some("v".into());
        let b = n("AXRow", "b").kids(vec![c]);
        let r0 = Revision::initial(&[n("AXWindow", "W").kids(vec![a, b])]);
        let full = render_full(&r0);
        let sel = r0.selected_lines();
        assert_eq!(sel.len(), 2);
        for l in &sel { assert!(full.lines().any(|fl| fl == l), "{l} not a full-render line"); }
        assert_eq!(sel, vec!["  [2] AXRow \"a\"".to_string(), "    [4] AXCell \"c\" = \"v\"".to_string()]);
        // next turn: selection moves to b; ids stay stable and the flag follows the NEW snapshot
        let mut b2 = n("AXRow", "b"); b2.selected = true;
        let r1 = r0.next(&[n("AXWindow", "W").kids(vec![n("AXRow", "a"), b2.kids(vec![n("AXCell", "c").with_value("v")])])]);
        assert_eq!(r1.selected_lines(), vec!["  [3] AXRow \"b\"".to_string()]);
        // persisted state from before the field existed still loads (serde default)
        let old = r#"{"id":1,"role":"AXWindow","label":"W","value":null,"depth":0,"children":[]}"#;
        let e: Element = serde_json::from_str(old).unwrap();
        assert!(!e.selected);
    }

    fn n(role: &str, label: &str) -> RawNode {
        RawNode::new(role, label)
    }

    fn window(children: Vec<RawNode>) -> Vec<RawNode> {
        vec![n("AXWindow", "WinTarget").kids(children)]
    }

    #[test]
    fn initial_is_full_and_ids_are_monotonic() {
        let rev = Revision::initial(&window(vec![
            n("AXButton", "Press me"),
            n("AXTextField", "entry"),
        ]));
        let out = render_diff(&rev, false, &DiffOptions::default());
        match out {
            DiffOutput::Full(s) => {
                assert!(s.contains("[1] AXWindow \"WinTarget\""));
                assert!(s.contains("[2] AXButton \"Press me\""));
                assert!(s.contains("[3] AXTextField \"entry\""));
            }
            other => panic!("expected Full, got {:?}", other),
        }
    }

    #[test]
    fn no_change_short_circuits_with_app_name() {
        let a = Revision::initial(&window(vec![n("AXButton", "Press me")]));
        let b = a.next(&window(vec![n("AXButton", "Press me")]));
        let opts = DiffOptions { app_name: "WinTarget".into(), ..Default::default() };
        assert_eq!(
            render_diff(&b, true, &opts),
            DiffOutput::NoChange("There has been no change in the accessibility tree for WinTarget".into())
        );
    }

    #[test]
    fn value_change_marks_tilde_and_keeps_id() {
        let a = Revision::initial(&window(vec![n("AXTextField", "entry")]));
        // entry id is 2 (window=1). Change its value.
        let b = a.next(&window(vec![n("AXTextField", "entry").with_value("hello")]));
        let out = render_diff(&b, true, &DiffOptions::default());
        let s = out.text();
        assert!(s.starts_with(HDR_INCREMENTAL), "header missing: {s}");
        assert!(s.contains(LEGEND_INCREMENTAL));
        // stable id 2 retained, marked ~
        assert!(s.contains("~ [2] AXTextField \"entry\" = \"hello\""), "got:\n{s}");
        // unchanged window is NOT emitted
        assert!(!s.contains("AXWindow"), "unchanged node leaked into diff:\n{s}");
    }

    #[test]
    fn added_node_gets_next_id_and_plus() {
        let a = Revision::initial(&window(vec![n("AXButton", "Press me")])); // window=1, button=2
        let b = a.next(&window(vec![n("AXButton", "Press me"), n("AXTextField", "entry")]));
        let out = render_diff(&b, true, &DiffOptions::default());
        let s = out.text();
        assert!(s.contains("+ [3] AXTextField \"entry\""), "got:\n{s}");
    }

    #[test]
    fn removed_node_incremental_dash_line() {
        let a = Revision::initial(&window(vec![n("AXButton", "Press me"), n("AXTextField", "entry")]));
        let b = a.next(&window(vec![n("AXButton", "Press me")])); // entry (id 3) removed
        let out = render_diff(&b, true, &DiffOptions::default());
        let s = out.text();
        assert!(s.contains("- [3]"), "expected removed dash line, got:\n{s}");
    }

    #[test]
    fn removed_ids_compacted_in_cumulative_mode() {
        // build a tree with 5 removable leaves (ids 2..=6), remove ids 2,3,4 and 6.
        let a = Revision::initial(&window(vec![
            n("AXRow", "r1"), // 2
            n("AXRow", "r2"), // 3
            n("AXRow", "r3"), // 4
            n("AXRow", "r4"), // 5
            n("AXRow", "r5"), // 6
        ]));
        let b = a.next(&window(vec![n("AXRow", "r4")])); // keep only r4 (id 5)
        // cumulative diff vs the INITIAL revision `a`.
        let out = render_cumulative(&b, &a, &DiffOptions::default());
        let s = out.text();
        assert!(s.starts_with(HDR_CUMULATIVE), "cumulative header missing:\n{s}");
        assert!(s.contains(LEGEND_CUMULATIVE));
        // removed 2,3,4 and 6 -> "2-4, 6"
        assert!(s.contains("Removed elements: 2-4, 6"), "got:\n{s}");
    }

    // Regression guard for the cumulative-baseline bug: cumulative must diff
    // vs the INITIAL tree, so a change made two turns ago still shows, whereas
    // the incremental diff (vs the previous turn) does not.
    #[test]
    fn cumulative_is_from_initial_not_previous() {
        // turn 1: two fields A (id2) and B (id3).
        let r1 = Revision::initial(&window(vec![
            n("AXTextField", "A"),
            n("AXTextField", "B"),
        ]));
        // turn 2: A gets a value (changed two turns before the end).
        let r2 = r1.next(&window(vec![
            n("AXTextField", "A").with_value("x"),
            n("AXTextField", "B"),
        ]));
        // turn 3: B gets a value; A is unchanged since turn 2.
        let r3 = r2.next(&window(vec![
            n("AXTextField", "A").with_value("x"),
            n("AXTextField", "B").with_value("y"),
        ]));

        // Incremental (vs turn 2): only B changed this turn; A must NOT appear.
        let inc = render_diff(&r3, true, &DiffOptions::default());
        let si = inc.text();
        assert!(si.contains("~ [3] AXTextField \"B\" = \"y\""), "incremental missing B:\n{si}");
        assert!(!si.contains("[2] AXTextField \"A\""), "incremental leaked A (unchanged this turn):\n{si}");

        // Cumulative (vs initial turn 1): BOTH A and B changed since the start.
        let cum = render_cumulative(&r3, &r1, &DiffOptions::default());
        let sc = cum.text();
        assert!(sc.starts_with(HDR_CUMULATIVE), "cumulative header:\n{sc}");
        assert!(sc.contains("~ [2] AXTextField \"A\" = \"x\""), "cumulative missing A:\n{sc}");
        assert!(sc.contains("~ [3] AXTextField \"B\" = \"y\""), "cumulative missing B:\n{sc}");
    }

    #[test]
    fn budget_exceeded_falls_back_to_full() {
        let a = Revision::initial(&window(vec![n("AXButton", "b")]));
        // add 5 new rows but set budget to 3 -> fallback
        let b = a.next(&window(vec![
            n("AXButton", "b"),
            n("AXRow", "1"),
            n("AXRow", "2"),
            n("AXRow", "3"),
            n("AXRow", "4"),
            n("AXRow", "5"),
        ]));
        let opts = DiffOptions { line_budget: 3, ..Default::default() };
        match render_diff(&b, true, &opts) {
            DiffOutput::FullFallback(s) => assert!(s.contains("AXWindow")),
            other => panic!("expected FullFallback, got {:?}", other),
        }
    }

    #[test]
    fn disable_diff_returns_full() {
        let a = Revision::initial(&window(vec![n("AXTextField", "entry")]));
        let b = a.next(&window(vec![n("AXTextField", "entry").with_value("x")]));
        let opts = DiffOptions { disable_diff: true, ..Default::default() };
        match render_diff(&b, true, &opts) {
            DiffOutput::Full(s) => assert!(s.contains("[2] AXTextField \"entry\" = \"x\"")),
            other => panic!("expected Full, got {:?}", other),
        }
    }

    #[test]
    fn ids_stay_stable_across_three_turns() {
        let a = Revision::initial(&window(vec![n("AXButton", "b")])); // window 1, button 2
        let b = a.next(&window(vec![n("AXButton", "b"), n("AXTextField", "t")])); // +field id 3
        let c = b.next(&window(vec![
            n("AXButton", "b").with_value("pressed"), // ~ still id 2
            n("AXTextField", "t"),
        ]));
        let out = render_diff(&c, true, &DiffOptions::default());
        let s = out.text();
        assert!(s.contains("~ [2] AXButton \"b\" = \"pressed\""), "id not stable across turns:\n{s}");
    }

    // The stateful layer persists a revision as (root, next_id) between calls.
    // Restoring and stepping must be identical to stepping the live revision,
    // so ids stay stable across a process boundary (serialize -> restore -> next).
    #[test]
    fn persist_restore_roundtrip_keeps_ids_stable() {
        let r1 = Revision::initial(&window(vec![n("AXButton", "b")])); // window 1, button 2
        // round-trip r1 through its persisted state
        let (root, next_id) = r1.state();
        let json = serde_json::to_string(root).unwrap();
        let root2: Vec<Element> = serde_json::from_str(&json).unwrap();
        let restored = Revision::restore(root2, next_id);
        // stepping the restored revision must match stepping the original
        let t2 = window(vec![n("AXButton", "b").with_value("x"), n("AXTextField", "t")]);
        let live = r1.next(&t2);
        let via_disk = restored.next(&t2);
        let d_live = render_diff(&live, true, &DiffOptions::default()).text().to_string();
        let d_disk = render_diff(&via_disk, true, &DiffOptions::default()).text().to_string();
        assert_eq!(d_live, d_disk, "restored-from-disk diff differs from live diff");
        assert!(d_disk.contains("~ [2] AXButton \"b\" = \"x\""), "id 2 not stable across disk:\n{d_disk}");
        assert!(d_disk.contains("+ [3] AXTextField \"t\""), "new id 3 wrong across disk:\n{d_disk}");
    }

    // Characterization of a KNOWN limitation (documented, not a defect to fix
    // blind): siblings that share a match key and carry no distinguishing label
    // are paired POSITIONALLY. So a reordering list of keyless rows shows up as
    // per-position value changes, not moves. This mirrors Codex's path/
    // depth-first-order pairing as far as the decompile shows; the exact Swift
    // heuristic is unverifiable without a real trace (account-gated golden).
    // If a future change alters this behavior, this test flags it for a
    // deliberate decision rather than a silent regression.
    #[test]
    fn indistinguishable_siblings_report_ambiguous_replacement() {
        let mk = |v: &str| RawNode::new("AXRow", "").with_value(v); // no label -> key (AXRow,"")
        let a = Revision::initial(&window(vec![mk("1"), mk("2"), mk("3")])); // ids 2,3,4
        // reorder values 1,2,3 -> 3,1,2 (same roles/labels, no stable label)
        let b = a.next(&window(vec![mk("3"), mk("1"), mk("2")]));
        let out = render_diff(&b, true, &DiffOptions::default());
        let s = out.text();
        // Neither a mutable value nor a same-role position proves identity.
        for id in 2..=4 { assert!(s.contains(&format!("- [{id}]")), "{s}"); }
        for (id, value) in [(5, "3"), (6, "1"), (7, "2")] {
            assert!(s.contains(&format!("+ [{id}] AXRow = \"{value}\"")), "{s}");
        }
        assert!(!s.contains("~ ["), "ambiguous rows must not reuse IDs: {s}");
    }

    // Captured on macOS 15 (study/audit/macos-native-2026-09-25, identity run):
    // a window's two unlabelled title-bar buttons have no identifier, so a value
    // change elsewhere re-minted both. Unchanged content at an unchanged
    // window-relative frame now keeps their ids.
    #[test]
    fn unchanged_anonymous_siblings_keep_ids_in_place() {
        let tree = |result: &str| window(vec![
            n("AXTextField", "Result").with_value(result),
            n("AXButton", "").at([30, 3, 14, 16]),
            n("AXButton", "").at([50, 3, 14, 16]),
        ]);
        let a = Revision::initial(&tree("0")); // window 1, field 2, buttons 3 and 4
        let b = a.next(&tree("42"));
        let s = render_diff(&b, true, &DiffOptions::default()).text().to_string();
        assert!(s.contains("~ [2] AXTextField \"Result\" = \"42\""), "{s}");
        assert!(!s.contains("+ [") && !s.contains("- ["), "unchanged buttons were re-minted: {s}");
        let ids: Vec<u64> = b.root[0].children.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![2, 3, 4]);
    }

    // The same evidence must not decide a removal, a move or a changed value.
    // Each turn also changes a field, so the identical-forest shortcut is off.
    #[test]
    fn frame_evidence_never_decides_removal_moves_or_changed_values() {
        let field = |v: &str| n("AXTextField", "Result").with_value(v);
        let btn = |x: i64| n("AXButton", "").at([x, 0, 20, 20]);
        let ids = |r: &Revision| r.root[0].children[1..].iter().map(|e| e.id).collect::<Vec<_>>();
        let a = Revision::initial(&window(vec![field("0"), btn(0), btn(30)])); // field 2, buttons 3, 4
        // One removed: the group shrank, so the survivor proves nothing.
        assert_eq!(ids(&a.next(&window(vec![field("1"), btn(30)]))), vec![5]);
        // Both moved: frames no longer match.
        assert_eq!(ids(&a.next(&window(vec![field("1"), btn(5), btn(35)]))), vec![5, 6]);
        // Values changed in place (a reorder of anonymous rows looks like this).
        let row = |y: i64, v: &str| n("AXRow", "").with_value(v).at([0, y, 50, 10]);
        let d = Revision::initial(&window(vec![field("0"), row(0, "1"), row(10, "2")]));
        assert_eq!(ids(&d.next(&window(vec![field("1"), row(0, "2"), row(10, "1")]))), vec![5, 6]);
        // Two members at one frame are still ambiguous.
        let f = Revision::initial(&window(vec![field("0"), btn(0), btn(0)]));
        assert_eq!(ids(&f.next(&window(vec![field("1"), btn(0), btn(0)]))), vec![5, 6]);
        // Without frames nothing changes from before.
        let anon = || n("AXButton", "");
        let h = Revision::initial(&window(vec![field("0"), anon(), anon()]));
        assert_eq!(ids(&h.next(&window(vec![field("1"), anon(), anon()]))), vec![5, 6]);
    }

    // The review's asymmetric collisions (review-2026-09-25, macos.md): equal
    // group sizes, but one frame holds one member on one side and two on the
    // other. Neither direction may hand the old id to a member, so dropping
    // either uniqueness check (`rivals != 1`, `candidates.len() != 1`) fails here.
    #[test]
    fn review_asymmetric_frame_collisions_stay_ambiguous() {
        let field = |v: &str| n("AXTextField", "Result").with_value(v);
        let btn = |x: i64| n("AXButton", "").at([x, 0, 20, 20]);
        let ids = |r: &Revision| r.root[0].children[1..].iter().map(|e| e.id).collect::<Vec<_>>();
        // One old member on frame 0, two new members on it: rivals == 2.
        let a = Revision::initial(&window(vec![field("0"), btn(0), btn(30)])); // buttons 3, 4
        assert_eq!(ids(&a.next(&window(vec![field("1"), btn(0), btn(0)]))), vec![5, 6]);
        // Two old members on frame 0, one new member on it: two candidates.
        let b = Revision::initial(&window(vec![field("0"), btn(0), btn(0)])); // buttons 3, 4
        assert_eq!(ids(&b.next(&window(vec![field("1"), btn(0), btn(30)]))), vec![5, 6]);
    }

    // A guard made to fail on purpose once (per the project's testing rule): if
    // the range compactor ever stops consolidating consecutive ids, this breaks.
    #[test]
    fn range_compaction_guard() {
        assert_eq!(compact_ranges(&[1, 2, 3, 5, 7, 8]), "1-3, 5, 7-8");
        assert_eq!(compact_ranges(&[42]), "42");
        assert_eq!(compact_ranges(&[]), "");
    }
}
