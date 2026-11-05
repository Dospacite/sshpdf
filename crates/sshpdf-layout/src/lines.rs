//! The line layer: thin horizontal and vertical lines (table rules, boxes,
//! underlines) drawn with box-drawing characters snapped to the cell grid.

use std::collections::{HashMap, HashSet};

use sshpdf_doc::{PageGraphics, RectPt, Rgb};

use crate::boxchars::{self, DOUBLE, DOWN, HEAVY, LEFT, LIGHT, RIGHT, UP, box_char};
use crate::mapping::Mapping;

/// A straight, axis-aligned line in page space.
#[derive(Clone, Debug, PartialEq)]
struct Seg {
    horizontal: bool,
    /// y for horizontal lines, x for vertical ones.
    pos: f32,
    /// Extent along the line.
    a: f32,
    b: f32,
    width: f32,
    color: Rgb,
    dashed: bool,
    double: bool,
    /// Indices (into the collected boxes) of the subpaths it draws: more
    /// than one after merging doubles or collinear pieces.
    sources: Vec<usize>,
}

/// One cell of the line layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineCell {
    pub ch: char,
    pub color: Rgb,
    /// Only horizontal arms (can become an underline under text).
    pub horizontal_only: bool,
}

/// Line layer of a page at one zoom.
#[derive(Clone, Debug, Default)]
pub struct LineLayer {
    /// Box-drawing characters by (col, row) of the page grid.
    pub cells: HashMap<(i32, i32), LineCell>,
    /// Page-space bounding boxes of the subpaths drawn here (exclusions for
    /// the raster layer).
    pub excluded: Vec<RectPt>,
    /// Rules that fell on rows of text, drawn as an underline of those cells.
    pub underlines: HashMap<(i32, i32), Rgb>,
}

const AXIS_TOL: f32 = 0.05;

/// Collect thin straight lines from the page's paths. A subpath counts only
/// if *all* its segments qualify; otherwise it stays with the raster layer
/// (so nothing gets drawn twice).
fn collect(graphics: &PageGraphics, m: &Mapping) -> (Vec<Seg>, Vec<RectPt>) {
    let max_thick = (1.5 * m.col_pitch()).min(0.5 * m.row_pitch);
    let mut segs = Vec::new();
    let mut boxes = Vec::new();
    for item in &graphics.paths {
        match (item.fill, item.stroke) {
            // Stroked lines (not filled, so the subpath can be removed whole).
            (None, Some(stroke)) => {
                if stroke.width > max_thick {
                    continue;
                }
                for sp in &item.subpaths {
                    if sp.curved || sp.points.len() < 2 {
                        continue;
                    }
                    let mut pts = sp.points.clone();
                    if sp.closed && pts.first() != pts.last() {
                        pts.push(pts[0]);
                    }
                    let mut mine = Vec::new();
                    let mut ok = true;
                    for w in pts.windows(2) {
                        let (p, q) = (w[0], w[1]);
                        let (dx, dy) = ((q.x - p.x).abs(), (q.y - p.y).abs());
                        if dx < 1e-3 && dy < 1e-3 {
                            continue;
                        }
                        let source = boxes.len();
                        if dy <= AXIS_TOL.max(dx * 0.01) {
                            mine.push(Seg {
                                horizontal: true,
                                pos: (p.y + q.y) / 2.0,
                                a: p.x.min(q.x),
                                b: p.x.max(q.x),
                                width: stroke.width,
                                color: stroke.color,
                                dashed: stroke.dashed,
                                double: false,
                                sources: vec![source],
                            });
                        } else if dx <= AXIS_TOL.max(dy * 0.01) {
                            mine.push(Seg {
                                horizontal: false,
                                pos: (p.x + q.x) / 2.0,
                                a: p.y.min(q.y),
                                b: p.y.max(q.y),
                                width: stroke.width,
                                color: stroke.color,
                                dashed: stroke.dashed,
                                double: false,
                                sources: vec![source],
                            });
                        } else {
                            ok = false;
                            break;
                        }
                    }
                    if ok && !mine.is_empty() {
                        boxes.push(sp.bbox);
                        segs.extend(mine);
                    }
                }
            }
            // Thin filled rectangles are rules (TeX draws \hline this way).
            (Some(color), None) => {
                for sp in &item.subpaths {
                    if sp.curved || !is_axis_rect(&sp.points) {
                        continue;
                    }
                    let r = sp.bbox;
                    let (w, h) = (r.width(), r.height());
                    let source = boxes.len();
                    let seg = if h <= max_thick && w >= h * 3.0 {
                        Seg {
                            horizontal: true,
                            pos: (r.y0 + r.y1) / 2.0,
                            a: r.x0,
                            b: r.x1,
                            width: h.max(0.1),
                            color,
                            dashed: false,
                            double: false,
                            sources: vec![source],
                        }
                    } else if w <= max_thick && h >= w * 3.0 {
                        Seg {
                            horizontal: false,
                            pos: (r.x0 + r.x1) / 2.0,
                            a: r.y0,
                            b: r.y1,
                            width: w.max(0.1),
                            color,
                            dashed: false,
                            double: false,
                            sources: vec![source],
                        }
                    } else {
                        continue;
                    };
                    boxes.push(r);
                    segs.push(seg);
                }
            }
            _ => {}
        }
    }
    (segs, boxes)
}

fn is_axis_rect(points: &[sshpdf_doc::PointPt]) -> bool {
    let mut pts: Vec<_> = points.to_vec();
    if pts.len() == 5 && (pts[0].x - pts[4].x).abs() < 1e-3 && (pts[0].y - pts[4].y).abs() < 1e-3 {
        pts.pop();
    }
    if pts.len() != 4 {
        return false;
    }
    (0..4).all(|i| {
        let (p, q) = (pts[i], pts[(i + 1) % 4]);
        (p.x - q.x).abs() < AXIS_TOL || (p.y - q.y).abs() < AXIS_TOL
    })
}

/// Merge pairs of close parallel lines into one double line.
fn find_doubles(segs: &mut Vec<Seg>, m: &Mapping) {
    let mut gone = vec![false; segs.len()];
    for i in 0..segs.len() {
        if gone[i] {
            continue;
        }
        for j in i + 1..segs.len() {
            if gone[j] || segs[i].horizontal != segs[j].horizontal {
                continue;
            }
            let (s, t) = (segs[i].clone(), segs[j].clone());
            let gap = (s.pos - t.pos).abs();
            let pitch = if s.horizontal { m.row_pitch } else { m.col_pitch() };
            // TeX's \\hline\\hline puts 0.4 pt rules 2.4 pt apart.
            let max_gap = (6.0 * s.width.max(t.width)).max(3.0).min(0.45 * pitch);
            let overlap = s.b.min(t.b) - s.a.max(t.a);
            let len = (s.b - s.a).min(t.b - t.a);
            if !s.double && !t.double && gap > 0.05 && gap <= max_gap && overlap > 0.8 * len {
                segs[i].pos = (s.pos + t.pos) / 2.0;
                segs[i].a = s.a.min(t.a);
                segs[i].b = s.b.max(t.b);
                segs[i].double = true;
                // The partner's subpath is drawn by this double line too.
                segs[i].sources.extend(t.sources);
                gone[j] = true;
                break;
            }
        }
    }
    let mut g = gone.iter();
    segs.retain(|_| !*g.next().expect("same length"));
}

/// Join collinear pieces of one rule (tables often draw a rule per column)
/// so the whole rule is placed in one row.
fn merge_collinear(segs: &mut Vec<Seg>) {
    segs.sort_by(|a, b| {
        (a.horizontal, a.double, a.dashed)
            .cmp(&(b.horizontal, b.double, b.dashed))
            .then(a.pos.total_cmp(&b.pos))
            .then(a.a.total_cmp(&b.a))
    });
    let mut out: Vec<Seg> = Vec::with_capacity(segs.len());
    for s in segs.drain(..) {
        if let Some(last) = out.last_mut()
            && last.horizontal == s.horizontal
            && last.double == s.double
            && last.dashed == s.dashed
            && (last.pos - s.pos).abs() < 0.6
            && s.a <= last.b + 1.5
            && (s.width / last.width).max(last.width / s.width) < 1.6
        {
            last.b = last.b.max(s.b);
            last.sources.extend(s.sources);
            continue;
        }
        out.push(s);
    }
    *segs = out;
}

/// Where a horizontal line was put.
#[derive(Clone, Copy, Debug)]
enum Lane {
    /// Drawn with box characters in this row.
    Row(i32),
    /// Drawn as an underline of this row of text.
    Underline(i32),
}

/// Where a line end lands.
enum End {
    /// On a perpendicular line, in this cell (column or row).
    Junction(i32),
    /// Between cells, at this boundary.
    Boundary(i32),
}

/// Build the line layer for a page at a mapping. `text` holds the cells the
/// text layer uses; rules avoid them.
pub fn line_layer(graphics: &PageGraphics, m: &Mapping, text: &HashSet<(i32, i32)>) -> LineLayer {
    let (mut segs, boxes) = collect(graphics, m);
    if segs.is_empty() {
        return LineLayer::default();
    }
    find_doubles(&mut segs, m);
    merge_collinear(&mut segs);

    // Weight: light, or heavy for lines clearly thicker than the thinnest.
    let thinnest = segs.iter().map(|s| s.width).fold(f32::MAX, f32::min);
    let weight = |s: &Seg| {
        if s.double {
            DOUBLE
        } else if s.width >= (1.5 * thinnest).max(0.7) {
            HEAVY
        } else {
            LIGHT
        }
    };

    let cp = m.col_pitch();
    let rp = m.row_pitch;
    // A line end within this distance of a perpendicular line joins it (an
    // arm to the centre of that line's cell); other ends round to the
    // nearest cell boundary, so free-standing rules cover whole cells.
    let snap_x = (0.45 * cp).max(1.0);
    let snap_y = (0.45 * rp).max(1.0);
    let verticals: Vec<Seg> = segs.iter().filter(|s| !s.horizontal).cloned().collect();
    let horizontals: Vec<Seg> = segs.iter().filter(|s| s.horizontal).cloned().collect();

    #[derive(Clone, Copy, Default)]
    struct Acc {
        arms: [u8; 4],
        color: Rgb,
        dashed: bool,
        any: bool,
    }
    let mut grid: HashMap<(i32, i32), Acc> = HashMap::new();
    let mut put = |col: i32, row: i32, dir: usize, s: &Seg| {
        let e = grid.entry((col, row)).or_default();
        e.arms[dir] = e.arms[dir].max(weight(s));
        if !e.any {
            e.color = s.color;
            e.dashed = s.dashed;
            e.any = true;
        } else {
            e.dashed &= s.dashed;
        }
    };
    let mut drawn = vec![false; boxes.len()];
    let mut underlines: HashMap<(i32, i32), Rgb> = HashMap::new();

    // Cells a segment covers along its length: (first full cell, half arm at
    // the start, last full cell, half arm at the end).
    let span = |s: &Seg, end_of: &dyn Fn(f32) -> End| {
        let (first_full, start_half) = match end_of(s.a) {
            End::Junction(c) => (c + 1, Some(c)),
            End::Boundary(b) => (b, None),
        };
        let (last_full, end_half) = match end_of(s.b) {
            End::Junction(c) => (c - 1, Some(c)),
            End::Boundary(b) => (b - 1, None),
        };
        (first_full, start_half, last_full, end_half)
    };

    // 1. Horizontal lines: pick a row. A rule in a row that holds text moves
    // to the free neighbouring row on its side, or else becomes an underline
    // of the text row above it.
    let mut lanes: Vec<Lane> = Vec::with_capacity(horizontals.len());
    for s in &horizontals {
        let end_of = |v: f32| -> End {
            for o in &verticals {
                if (o.pos - v).abs() <= snap_x && s.pos >= o.a - snap_x && s.pos <= o.b + snap_x {
                    return End::Junction(m.col_of(o.pos).floor() as i32);
                }
            }
            End::Boundary(m.col_of(v).round() as i32)
        };
        let (first_full, start_half, last_full, end_half) = span(s, &end_of);
        let c_lo = start_half.unwrap_or(first_full);
        let c_hi = end_half.unwrap_or(last_full);
        let f = m.row_of(s.pos);
        let r0 = f.floor() as i32;
        let below = f - f.floor() >= 0.5;
        let has_text = |row: i32| (c_lo..=c_hi).any(|c| text.contains(&(c, row)));
        let lane = if !has_text(r0) {
            Lane::Row(r0)
        } else {
            let alt = if below { r0 + 1 } else { r0 - 1 };
            if !has_text(alt) {
                Lane::Row(alt)
            } else {
                // Underline the text the rule runs under.
                let row = if below { r0 } else { r0 - 1 };
                if c_hi >= c_lo {
                    for c in c_lo..=c_hi {
                        underlines.entry((c, row)).or_insert(s.color);
                    }
                    for &k in &s.sources {
                        drawn[k] = true;
                    }
                }
                Lane::Underline(row)
            }
        };
        lanes.push(lane);
    }

    // 2. Draw both directions; vertical lines end at the rows their
    // horizontal partners ended up in.
    let h_with_lanes: Vec<(Seg, Lane)> = horizontals.iter().cloned().zip(lanes.iter().copied()).collect();
    for s in &segs {
        let (pitch, along): (f32, &dyn Fn(f32) -> f32) = if s.horizontal {
            (cp, &|x| m.col_of(x))
        } else {
            (rp, &|y| m.row_of(y))
        };
        let lane = if s.horizontal {
            match h_with_lanes.iter().find(|(h, _)| h == s) {
                Some((_, Lane::Row(l))) => *l,
                // Became an underline.
                _ => continue,
            }
        } else {
            m.col_of(s.pos).floor() as i32
        };
        let end_of = |v: f32| -> End {
            if s.horizontal {
                for o in &verticals {
                    if (o.pos - v).abs() <= snap_x && s.pos >= o.a - snap_x && s.pos <= o.b + snap_x {
                        return End::Junction(m.col_of(o.pos).floor() as i32);
                    }
                }
                End::Boundary(m.col_of(v).round() as i32)
            } else {
                for (o, l) in &h_with_lanes {
                    if (o.pos - v).abs() <= snap_y && s.pos >= o.a - snap_y && s.pos <= o.b + snap_y {
                        return match l {
                            Lane::Row(l) => End::Junction(*l),
                            // The partner became an underline at the bottom
                            // of its row: end at that cell boundary.
                            Lane::Underline(r) => End::Boundary(*r + 1),
                        };
                    }
                }
                End::Boundary(m.row_of(v).round() as i32)
            }
        };
        let (first_full, start_half, last_full, end_half) = span(s, &end_of);
        // (lane, position along the line) -> (col, row)
        let at = |along: i32| if s.horizontal { (along, lane) } else { (lane, along) };
        let (back, fwd) = if s.horizontal { (LEFT, RIGHT) } else { (UP, DOWN) };
        match (start_half, end_half) {
            (Some(a), Some(b)) if a >= b => continue,
            (None, None) if last_full < first_full => {
                // Shorter than a cell: one cell if it is long enough.
                if s.b - s.a < 0.6 * pitch {
                    continue;
                }
                let (c, r) = at(along((s.a + s.b) / 2.0).floor() as i32);
                put(c, r, back, s);
                put(c, r, fwd, s);
            }
            _ => {
                if let Some(a) = start_half {
                    let (c, r) = at(a);
                    put(c, r, fwd, s);
                }
                for k in first_full..=last_full {
                    let (c, r) = at(k);
                    put(c, r, back, s);
                    put(c, r, fwd, s);
                }
                if let Some(b) = end_half {
                    let (c, r) = at(b);
                    put(c, r, back, s);
                }
            }
        }
        for &k in &s.sources {
            drawn[k] = true;
        }
    }

    let mut layer = LineLayer {
        underlines,
        ..LineLayer::default()
    };
    for ((col, row), acc) in grid {
        let horizontal_only = acc.arms[UP] == boxchars::NONE && acc.arms[DOWN] == boxchars::NONE;
        let vertical_only = acc.arms[LEFT] == boxchars::NONE && acc.arms[RIGHT] == boxchars::NONE;
        let ch = if acc.dashed && horizontal_only {
            '┄'
        } else if acc.dashed && vertical_only {
            '┆'
        } else {
            match box_char(acc.arms) {
                Some(c) => c,
                None => continue,
            }
        };
        layer.cells.insert(
            (col, row),
            LineCell {
                ch,
                color: acc.color,
                horizontal_only,
            },
        );
    }
    // A subpath whose segments were all too short to draw stays raster.
    layer.excluded = boxes
        .into_iter()
        .zip(drawn)
        .filter_map(|(b, d)| d.then_some(b))
        .collect();
    layer
}

#[cfg(test)]
mod tests {
    use super::*;
    use sshpdf_doc::{PathItem, PointPt, StrokeStyle, SubPath};

    fn stroke(points: &[(f32, f32)], closed: bool, width: f32) -> PathItem {
        let points: Vec<PointPt> = points.iter().map(|&(x, y)| PointPt { x, y }).collect();
        let bbox = points.iter().fold(
            RectPt::new(points[0].x, points[0].y, points[0].x, points[0].y),
            |r, p| r.union(&RectPt::new(p.x, p.y, p.x, p.y)),
        );
        PathItem {
            subpaths: vec![SubPath {
                points,
                closed,
                curved: false,
                bbox,
            }],
            stroke: Some(StrokeStyle {
                width,
                color: [0, 0, 0],
                dashed: false,
            }),
            fill: None,
        }
    }

    fn render(layer: &LineLayer, cols: i32, rows: i32) -> Vec<String> {
        (0..rows)
            .map(|r| {
                (0..cols)
                    .map(|c| layer.cells.get(&(c, r)).map_or(' ', |l| l.ch))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_box_with_a_cross() {
        // 10 pt rows, 5 pt columns. A box from (2.5, 5) to (27.5, 35) with a
        // middle horizontal rule at y = 15 and a vertical one at x = 12.5.
        let m = Mapping::new(10.0, 0.5);
        let g = PageGraphics {
            paths: vec![
                stroke(&[(2.5, 5.0), (27.5, 5.0), (27.5, 35.0), (2.5, 35.0)], true, 0.5),
                stroke(&[(2.5, 15.0), (27.5, 15.0)], false, 0.5),
                stroke(&[(12.5, 5.0), (12.5, 35.0)], false, 0.5),
            ],
            images: vec![],
        };
        let l = line_layer(&g, &m, &HashSet::new());
        assert_eq!(render(&l, 6, 4), vec!["┌─┬──┐", "├─┼──┤", "│ │  │", "└─┴──┘"]);
        assert_eq!(l.excluded.len(), 3);
    }

    #[test]
    fn overshooting_rules_still_join() {
        let m = Mapping::new(10.0, 0.5);
        // The horizontal rule starts 1.2 pt left of the vertical one.
        let g = PageGraphics {
            paths: vec![
                stroke(&[(9.8, 5.0), (30.0, 5.0)], false, 0.4),
                stroke(&[(11.0, 5.0), (11.0, 25.0)], false, 0.4),
            ],
            images: vec![],
        };
        let l = line_layer(&g, &m, &HashSet::new());
        // The free right end rounds to the cell boundary at x = 30.
        assert_eq!(render(&l, 7, 3)[0], "  ┌─── ");
    }

    #[test]
    fn diagonal_subpaths_stay_raster() {
        let m = Mapping::new(10.0, 0.5);
        let g = PageGraphics {
            paths: vec![stroke(&[(0.0, 0.0), (20.0, 0.0), (30.0, 20.0)], false, 0.5)],
            images: vec![],
        };
        let l = line_layer(&g, &m, &HashSet::new());
        assert!(l.cells.is_empty() && l.excluded.is_empty());
    }

    #[test]
    fn double_rules_and_heavy_rules() {
        let m = Mapping::new(10.0, 0.5);
        let g = PageGraphics {
            paths: vec![
                stroke(&[(0.0, 4.0), (20.0, 4.0)], false, 0.4),
                stroke(&[(0.0, 6.0), (20.0, 6.0)], false, 0.4),
                stroke(&[(0.0, 25.0), (20.0, 25.0)], false, 1.5),
            ],
            images: vec![],
        };
        let l = line_layer(&g, &m, &HashSet::new());
        let r = render(&l, 5, 3);
        assert_eq!(r[0], "════ ");
        assert_eq!(r[2], "━━━━ ");
        assert_eq!(l.excluded.len(), 3);
    }

    #[test]
    fn rules_avoid_text_rows() {
        let m = Mapping::new(10.0, 0.5);
        // Two rules: at y = 14 (row 1, upper half) and y = 26 (row 2, lower
        // half). Text sits on rows 1 and 2; row 0 is free, row 3 too.
        let g = PageGraphics {
            paths: vec![
                stroke(&[(0.0, 14.0), (20.0, 14.0)], false, 0.5),
                stroke(&[(0.0, 26.0), (20.0, 26.0)], false, 0.5),
            ],
            images: vec![],
        };
        let text: HashSet<(i32, i32)> = (0..4).flat_map(|c| [(c, 1), (c, 2)]).collect();
        let l = line_layer(&g, &m, &text);
        let r = render(&l, 4, 4);
        assert_eq!(r[0], "────");
        assert_eq!(r[3], "────");
        assert!(l.underlines.is_empty());
        // With every row taken, rules underline the text above them.
        let text: HashSet<(i32, i32)> = (0..4).flat_map(|c| (0..4).map(move |r| (c, r))).collect();
        let l = line_layer(&g, &m, &text);
        assert!(l.cells.is_empty());
        assert!(l.underlines.contains_key(&(0, 0)) && l.underlines.contains_key(&(0, 2)));
        assert_eq!(l.excluded.len(), 2);
    }

    #[test]
    fn rules_drawn_per_column_are_one_rule() {
        let m = Mapping::new(10.0, 0.5);
        // A rule drawn as three pieces, one per table column, crossing a
        // row with text in the last column only: it must not be drawn in
        // the first two columns and underline the third.
        let g = PageGraphics {
            paths: vec![
                stroke(&[(0.0, 26.0), (10.0, 26.0)], false, 0.5),
                stroke(&[(10.0, 26.0), (20.0, 26.0)], false, 0.5),
                stroke(&[(20.0, 26.0), (40.0, 26.0)], false, 0.5),
            ],
            images: vec![],
        };
        let text: HashSet<(i32, i32)> = (4..8).flat_map(|c| [(c, 2), (c, 3)]).collect();
        let l = line_layer(&g, &m, &text);
        assert!(l.cells.is_empty(), "{:?}", render(&l, 8, 4));
        assert_eq!(l.underlines.len(), 8);
        assert_eq!(l.excluded.len(), 3);
    }

    #[test]
    fn thin_filled_rects_are_rules() {
        let m = Mapping::new(10.0, 0.5);
        let pts = [(0.0, 14.8), (20.0, 14.8), (20.0, 15.2), (0.0, 15.2)];
        let mut item = stroke(&pts, true, 0.0);
        item.stroke = None;
        item.fill = Some([0, 0, 0]);
        let l = line_layer(
            &PageGraphics {
                paths: vec![item],
                images: vec![],
            },
            &m,
            &HashSet::new(),
        );
        assert_eq!(render(&l, 5, 2)[1], "──── ");
    }
}
