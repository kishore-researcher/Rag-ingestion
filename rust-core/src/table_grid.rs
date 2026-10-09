use crate::types::LineSegment;
use std::collections::HashMap;

/// Segments shorter than this (in points) are treated as noise (tick marks,
/// serif details in filled glyphs misclassified upstream, etc.) rather than
/// table rules.
const MIN_RULE_LENGTH_PT: f64 = 8.0;
/// How far off-axis (in points) a segment may be and still count as
/// "horizontal" or "vertical" rather than a diagonal/decorative line.
const AXIS_TOLERANCE_PT: f64 = 1.0;
/// How close two rules' positions (y for horizontal, x for vertical) need
/// to be to count as the same rule line rather than two different ones.
const CLUSTER_TOLERANCE_PT: f64 = 2.0;
/// A little slack applied when checking whether a horizontal and vertical
/// rule actually cross, to tolerate rounding/off-by-a-hair rendering.
const INTERSECTION_TOLERANCE_PT: f64 = 2.0;

/// Minimum distinct horizontal rules for a lattice: 3 (>= 2 rows of cells).
/// A lone closed rectangle (figure border, callout, flowchart node, page
/// frame) has 2 horizontal + 2 vertical edges that cross each other and
/// used to pass the old ">= 2 and >= 2" test as a 1x1 "table".
const MIN_HORIZONTAL_RULES: usize = 3;
/// Minimum distinct vertical rules: 2 (left and right borders).
const MIN_VERTICAL_RULES: usize = 2;

struct Rule {
    pos: f64,       // y for horizontal rules, x for vertical rules
    span_min: f64,  // x_min for horizontal, y_min for vertical
    span_max: f64,  // x_max for horizontal, y_max for vertical
}

fn cluster_rules(mut items: Vec<Rule>) -> Vec<Rule> {
    items.sort_by(|a, b| a.pos.partial_cmp(&b.pos).unwrap());
    let mut clustered: Vec<Rule> = Vec::new();
    for item in items {
        if let Some(last) = clustered.last_mut() {
            if (last.pos - item.pos).abs() <= CLUSTER_TOLERANCE_PT {
                // Merge: widen the span, average the position.
                last.span_min = last.span_min.min(item.span_min);
                last.span_max = last.span_max.max(item.span_max);
                last.pos = (last.pos + item.pos) / 2.0;
                continue;
            }
        }
        clustered.push(item);
    }
    clustered
}

/// Detects a ruled-table lattice from actual drawn vector lines: clusters
/// stroked/thin-filled segments into distinct horizontal and vertical rule
/// positions, keeps only rules that mutually intersect at least twice
/// (i.e. genuinely form a grid together, as opposed to an unrelated
/// horizontal divider elsewhere on the page), and returns `(rows, cols)`
/// when a real lattice of at least 2x2 rules is found.
///
/// This is a *complement* to, not a replacement for, the spatial-alignment
/// heuristic in fidelity.rs: it gives high-confidence detection for tables
/// that actually have drawn borders, but says nothing about the very common
/// case of a borderless table relying purely on column alignment (which
/// still needs the spatial heuristic). See README for why "100% accurate
/// for all tables" isn't achievable by rule-detection alone.
pub fn detect_ruled_grid(lines: &[LineSegment]) -> Option<(usize, usize)> {
    let mut horizontals = Vec::new();
    let mut verticals = Vec::new();

    for seg in lines {
        let dx = seg.x1 - seg.x0;
        let dy = seg.y1 - seg.y0;
        let length = (dx * dx + dy * dy).sqrt();
        if length < MIN_RULE_LENGTH_PT {
            continue;
        }
        if dy.abs() <= AXIS_TOLERANCE_PT && dx.abs() > dy.abs() {
            horizontals.push(Rule {
                pos: (seg.y0 + seg.y1) / 2.0,
                span_min: seg.x0.min(seg.x1),
                span_max: seg.x0.max(seg.x1),
            });
        } else if dx.abs() <= AXIS_TOLERANCE_PT && dy.abs() > dx.abs() {
            verticals.push(Rule {
                pos: (seg.x0 + seg.x1) / 2.0,
                span_min: seg.y0.min(seg.y1),
                span_max: seg.y0.max(seg.y1),
            });
        }
        // Diagonal segments are ignored entirely -- not rule-line candidates.
    }

    let mut h_rules = cluster_rules(horizontals);
    let mut v_rules = cluster_rules(verticals);
    if h_rules.len() < MIN_HORIZONTAL_RULES || v_rules.len() < MIN_VERTICAL_RULES {
        return None;
    }

    let intersects = |h: &Rule, v: &Rule| {
        v.pos >= h.span_min - INTERSECTION_TOLERANCE_PT
            && v.pos <= h.span_max + INTERSECTION_TOLERANCE_PT
            && h.pos >= v.span_min - INTERSECTION_TOLERANCE_PT
            && h.pos <= v.span_max + INTERSECTION_TOLERANCE_PT
    };

    // Keep only rules that participate in a genuine grid: a horizontal rule
    // must cross at least 2 vertical rules, and vice versa. One pass each
    // way is sufficient for the common case (a single rectangular ruled
    // table where every rule spans the full table).
    h_rules.retain(|h| v_rules.iter().filter(|v| intersects(h, v)).count() >= 2);
    v_rules.retain(|v| h_rules.iter().filter(|h| intersects(h, v)).count() >= 2);

    let n_h = h_rules.len();
    let n_v = v_rules.len();
    if n_h == 0 || n_v == 0 {
        return None;
    }

    // The per-rule filter does not check that survivors form ONE lattice:
    // several unrelated boxes each pass it, and counting globally would sum
    // them into a fake table. Split survivors into connected components
    // (rules linked when they cross) and judge each on its own.
    let mut parent: Vec<usize> = (0..n_h + n_v).collect();
    fn find(parent: &mut [usize], mut x: usize) -> usize {
        while parent[x] != x {
            parent[x] = parent[parent[x]];
            x = parent[x];
        }
        x
    }
    for (i, h) in h_rules.iter().enumerate() {
        for (j, v) in v_rules.iter().enumerate() {
            if intersects(h, v) {
                let (a, b) = (find(&mut parent, i), find(&mut parent, n_h + j));
                if a != b {
                    parent[a] = b;
                }
            }
        }
    }
    let mut counts: HashMap<usize, (usize, usize)> = HashMap::new();
    for i in 0..n_h {
        counts.entry(find(&mut parent, i)).or_default().0 += 1;
    }
    for j in 0..n_v {
        counts.entry(find(&mut parent, n_h + j)).or_default().1 += 1;
    }

    counts
        .values()
        .filter(|(h, v)| *h >= MIN_HORIZONTAL_RULES && *v >= MIN_VERTICAL_RULES)
        .max_by_key(|(h, v)| h * v)
        .map(|(h, v)| (h - 1, v - 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hline(x0: f64, x1: f64, y: f64) -> LineSegment {
        LineSegment { x0, y0: y, x1, y1: y }
    }
    fn vline(x: f64, y0: f64, y1: f64) -> LineSegment {
        LineSegment { x0: x, y0, x1: x, y1 }
    }

    #[test]
    fn detects_clean_3x3_ruled_grid() {
        // 4 horizontal rules x 4 vertical rules -> 3 rows x 3 cols.
        let mut lines = Vec::new();
        for y in [100.0, 80.0, 60.0, 40.0] {
            lines.push(hline(0.0, 150.0, y));
        }
        for x in [0.0, 50.0, 100.0, 150.0] {
            lines.push(vline(x, 40.0, 100.0));
        }
        assert_eq!(detect_ruled_grid(&lines), Some((3, 3)));
    }

    #[test]
    fn single_stray_divider_line_is_not_a_grid() {
        // Just one horizontal rule (e.g. a footer divider) -- no table here.
        let lines = vec![hline(0.0, 500.0, 700.0)];
        assert_eq!(detect_ruled_grid(&lines), None);
    }

    #[test]
    fn unrelated_lines_elsewhere_on_page_dont_form_a_false_grid() {
        // Two horizontal lines far apart with no verticals connecting them,
        // plus two short unrelated vertical tick marks elsewhere.
        let lines = vec![
            hline(0.0, 100.0, 700.0),
            hline(0.0, 100.0, 100.0),
            vline(300.0, 300.0, 320.0), // unrelated, doesn't intersect either horizontal
            vline(400.0, 300.0, 320.0),
        ];
        assert_eq!(detect_ruled_grid(&lines), None);
    }

    #[test]
    fn tiny_decorative_marks_are_ignored() {
        let lines = vec![hline(0.0, 3.0, 100.0), vline(0.0, 97.0, 100.0)]; // both under MIN_RULE_LENGTH_PT
        assert_eq!(detect_ruled_grid(&lines), None);
    }

    fn boxes(origins: &[(f64, f64)]) -> Vec<LineSegment> {
        let mut lines = Vec::new();
        for &(x0, y0) in origins {
            lines.push(hline(x0, x0 + 80.0, y0));
            lines.push(hline(x0, x0 + 80.0, y0 + 40.0));
            lines.push(vline(x0, y0, y0 + 40.0));
            lines.push(vline(x0 + 80.0, y0, y0 + 40.0));
        }
        lines
    }

    #[test]
    fn near_duplicate_rules_merge_into_one() {
        let mut lines = vec![
            hline(0.0, 150.0, 100.0),
            hline(0.0, 150.0, 99.3),
            hline(0.0, 150.0, 80.0),
            hline(0.0, 150.0, 60.0),
        ];
        lines.push(vline(0.0, 60.0, 100.0));
        lines.push(vline(150.0, 60.0, 100.0));
        assert_eq!(detect_ruled_grid(&lines), Some((2, 1)));
    }

    #[test]
    fn a_single_closed_rectangle_is_not_a_table() {
        assert_eq!(detect_ruled_grid(&boxes(&[(0.0, 0.0)])), None);
    }

    #[test]
    fn minimum_valid_table_is_three_horizontal_two_vertical() {
        let lines = vec![
            hline(0.0, 100.0, 100.0),
            hline(0.0, 100.0, 50.0),
            hline(0.0, 100.0, 0.0),
            vline(0.0, 0.0, 100.0),
            vline(100.0, 0.0, 100.0),
        ];
        assert_eq!(detect_ruled_grid(&lines), Some((2, 1)));
    }

    #[test]
    fn real_table_is_still_detected_alongside_unrelated_decorative_boxes() {
        let mut lines = vec![
            hline(0.0, 150.0, 100.0),
            hline(0.0, 150.0, 70.0),
            hline(0.0, 150.0, 40.0),
            hline(0.0, 150.0, 10.0),
            vline(0.0, 10.0, 100.0),
            vline(75.0, 10.0, 100.0),
            vline(150.0, 10.0, 100.0),
        ];
        lines.extend(boxes(&[(300.0, 500.0), (450.0, 600.0)]));
        assert_eq!(detect_ruled_grid(&lines), Some((3, 2)));
    }

    #[test]
    fn three_disconnected_boxes_do_not_sum_into_a_table() {
        let lines = boxes(&[(0.0, 0.0), (200.0, 150.0), (400.0, 300.0)]);
        assert_eq!(detect_ruled_grid(&lines), None);
    }
}
