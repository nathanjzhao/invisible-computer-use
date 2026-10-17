// Realistic three-turn session: initial full snapshot, then a diff after the
// user's action lands (textfield gets a value, a status row appears, a stale
// row is removed), then the cumulative removed-id-range variant.
use axdiff::*;

fn win(children: Vec<RawNode>) -> Vec<RawNode> {
    vec![RawNode::new("AXWindow", "Mail").kids(children)]
}

fn main() {
    // Turn 1: initial tree.
    let t1 = win(vec![
        RawNode::new("AXButton", "Send"),
        RawNode::new("AXTextField", "To"),
        RawNode::new("AXTextField", "Subject"),
        RawNode::new("AXRow", "draft-1"),
        RawNode::new("AXRow", "draft-2"),
    ]);
    let r1 = Revision::initial(&t1);
    println!("===== TURN 1 (initial: full tree, monotonic ids) =====");
    println!("{}\n", render_diff(&r1, false, &DiffOptions::default()).text());

    // Turn 2: agent typed into Subject; a StatusText appeared; draft-2 removed.
    let t2 = win(vec![
        RawNode::new("AXButton", "Send"),
        RawNode::new("AXTextField", "To"),
        RawNode::new("AXTextField", "Subject").with_value("Lunch?"),
        RawNode::new("AXStaticText", "Saving..."),
        RawNode::new("AXRow", "draft-1"),
    ]);
    let r2 = r1.next(&t2);
    let opts = DiffOptions { app_name: "Mail".into(), ..Default::default() };
    println!("===== TURN 2 (incremental diff) =====");
    println!("{}\n", render_diff(&r2, true, &opts).text());

    // Turn 3: cumulative variant with removed-id-range summary. Drop several rows.
    let t3 = win(vec![
        RawNode::new("AXButton", "Send"),
        RawNode::new("AXTextField", "Subject").with_value("Lunch?"),
    ]);
    let r3 = r2.next(&t3);
    let cum = DiffOptions { app_name: "Mail".into(), ..Default::default() };
    println!("===== TURN 3 (cumulative diff vs INITIAL, removed-as-ID-range) =====");
    println!("{}", render_cumulative(&r3, &r1, &cum).text());
}
