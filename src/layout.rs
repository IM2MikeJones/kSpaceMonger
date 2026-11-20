//! The box layout, following MECHANICS.md ("Layout").

use crate::scan::Tree;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x as f32 && py >= self.y as f32 && px < (self.x + self.w) as f32 && py < (self.y + self.h) as f32
    }
}

#[derive(Clone, Debug)]
pub struct Placed {
    /// None = gray filler standing for items too small to show.
    pub node: Option<usize>,
    pub rect: IRect,
    pub depth: i32,
    pub is_folder: bool,
    /// The folder's own children were laid out inside it.
    pub opened: bool,
}

/// Minimum (width, height) a region must exceed, by density -3..=2 (table 0x4249b8).
pub fn min_size(density: i32) -> (i32, i32) {
    const T: [(i32, i32); 6] = [(96, 64), (64, 48), (48, 32), (32, 24), (24, 16), (16, 12)];
    T[(density.clamp(-3, 2) + 3) as usize]
}

pub struct Params {
    pub density: i32,
    pub bias: i32,
    pub show_free: bool,
}

pub const INSET_LEFT: i32 = 3;
pub const INSET_TOP: i32 = 12;
pub const INSET_RIGHT: i32 = 3;
pub const INSET_BOTTOM: i32 = 3;

struct Ctx<'a> {
    tree: &'a Tree,
    min_w: i32,
    min_h: i32,
    wf: i64,
    hf: i64,
    show_free: bool,
    out: Vec<Placed>,
}

/// Lay out the view root's contents in a w×h area. Parents come before their children.
pub fn layout(tree: &Tree, view_root: usize, w: i32, h: i32, p: &Params) -> Vec<Placed> {
    let (min_w, min_h) = min_size(p.density);
    let b = p.bias.clamp(-20, 20) as i64;
    let mut cx = Ctx { tree, min_w, min_h, wf: 8 + b.max(0), hf: 8 + (-b).max(0), show_free: p.show_free, out: Vec::new() };
    let depth = tree.nodes[view_root].depth + 1;
    layout_folder(&mut cx, view_root, IRect { x: 0, y: 0, w, h }, depth);
    cx.out
}

fn layout_folder(cx: &mut Ctx, folder: usize, r: IRect, depth: i32) {
    if r.w <= 0 || r.h <= 0 {
        return;
    }
    let tree = cx.tree;
    let mut items: Vec<(usize, u64)> = tree.nodes[folder]
        .children
        .iter()
        .filter_map(|&c| {
            let s = tree.nodes[c].size;
            (s > 0).then_some((c, s))
        })
        .collect();
    if cx.show_free && tree.nodes[folder].parent.is_none() {
        if let Some(f) = tree.free_node {
            if tree.nodes[f].size > 0 {
                items.push((f, tree.nodes[f].size));
                items.sort_by(|a, b| b.1.cmp(&a.1));
            }
        }
    }
    if !items.is_empty() {
        split(cx, &items, r, depth);
    }
}

fn big(cx: &Ctx, r: IRect) -> bool {
    r.w > cx.min_w && r.h > cx.min_h
}

fn split(cx: &mut Ctx, items: &[(usize, u64)], r: IRect, depth: i32) {
    // greedy two-way partition (0x405aab): add to A unless sum(A) > sum(B)
    let mut a: Vec<(usize, u64)> = Vec::new();
    let mut b: Vec<(usize, u64)> = Vec::new();
    let (mut sa, mut sb) = (0u64, 0u64);
    for &it in items {
        if sa > sb {
            b.push(it);
            sb += it.1;
        } else {
            a.push(it);
            sa += it.1;
        }
    }
    let total = sa as u128 + sb as u128;
    if total == 0 {
        return;
    }
    // direction with bias (0x405b63)
    let (ra, rb) = if cx.wf * r.w as i64 / 8 > cx.hf * r.h as i64 / 8 {
        let wa = (r.w as u128 * sa as u128 / total) as i32;
        (IRect { x: r.x, y: r.y, w: wa, h: r.h }, IRect { x: r.x + wa, y: r.y, w: r.w - wa, h: r.h })
    } else {
        let ha = (r.h as u128 * sa as u128 / total) as i32;
        (IRect { x: r.x, y: r.y, w: r.w, h: ha }, IRect { x: r.x, y: r.y + ha, w: r.w, h: r.h - ha })
    };
    place_group(cx, &a, ra, depth);
    place_group(cx, &b, rb, depth);
}

fn place_group(cx: &mut Ctx, g: &[(usize, u64)], r: IRect, depth: i32) {
    if g.is_empty() {
        return;
    }
    if !big(cx, r) {
        cx.out.push(Placed { node: None, rect: r, depth, is_folder: false, opened: false });
        return;
    }
    if g.len() > 1 {
        split(cx, g, r, depth);
        return;
    }
    let idx = g[0].0;
    let n = &cx.tree.nodes[idx];
    let is_folder = n.is_dir && !n.is_free;
    let has_kids = is_folder && !n.children.is_empty();
    let slot = cx.out.len();
    cx.out.push(Placed { node: Some(idx), rect: r, depth, is_folder, opened: false });
    if has_kids {
        let inner = IRect {
            x: r.x + INSET_LEFT,
            y: r.y + INSET_TOP,
            w: r.w - INSET_LEFT - INSET_RIGHT,
            h: r.h - INSET_TOP - INSET_BOTTOM,
        };
        let before = cx.out.len();
        layout_folder(cx, idx, inner, depth + 1);
        cx.out[slot].opened = cx.out.len() > before;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{Node, Tree};
    use std::path::PathBuf;

    fn node(name: &str, size: u64, parent: Option<usize>, depth: i32, is_dir: bool) -> Node {
        Node {
            name: name.into(),
            size,
            is_dir,
            is_free: false,
            parent,
            children: Vec::new(),
            depth,
            mtime: None,
            mode: 0o644,
            files: 0,
            dirs: 0,
        }
    }

    /// Root with files of the given sizes (largest first).
    fn flat(sizes: &[u64]) -> Tree {
        let mut nodes = vec![node("/", sizes.iter().sum(), None, -1, true)];
        for (i, s) in sizes.iter().enumerate() {
            nodes.push(node(&format!("f{i}"), *s, Some(0), 0, false));
            nodes[0].children.push(i + 1);
        }
        Tree { nodes, root_path: PathBuf::from("/"), fs_total: 0, fs_free: 0, free_node: None }
    }

    fn p(density: i32, bias: i32) -> Params {
        Params { density, bias, show_free: false }
    }

    #[test]
    fn density_table_matches_original() {
        assert_eq!(min_size(-3), (96, 64));
        assert_eq!(min_size(0), (32, 24));
        assert_eq!(min_size(2), (16, 12));
    }

    #[test]
    fn two_equal_files_split_the_wider_side() {
        let t = flat(&[100, 100]);
        let out = layout(&t, 0, 400, 200, &p(0, 0));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].rect, IRect { x: 0, y: 0, w: 200, h: 200 });
        assert_eq!(out[1].rect, IRect { x: 200, y: 0, w: 200, h: 200 });
    }

    #[test]
    fn greedy_partition_and_proportional_cut() {
        // A gets 60, then 30 goes to B (60 > 0), then 10 goes to B (60 > 30): 60 | 40
        let t = flat(&[60, 30, 10]);
        let out = layout(&t, 0, 1000, 400, &p(0, 0));
        assert_eq!(out[0].rect, IRect { x: 0, y: 0, w: 600, h: 400 });
        // B (30+10) in 400x400 is cut across its height: 300 | 100
        assert_eq!(out[1].rect, IRect { x: 600, y: 0, w: 400, h: 300 });
        assert_eq!(out[2].rect, IRect { x: 600, y: 300, w: 400, h: 100 });
    }

    #[test]
    fn bias_favours_vertical_or_horizontal_cuts() {
        let t = flat(&[100, 100]);
        // square area: with no bias the height is cut (w*8/8 > h*8/8 is false)
        let out = layout(&t, 0, 300, 300, &p(0, 0));
        assert_eq!(out[0].rect.w, 300);
        // positive bias weights the width, so the width is cut
        let out = layout(&t, 0, 300, 300, &p(0, 5));
        assert_eq!(out[0].rect.h, 300);
    }

    #[test]
    fn small_regions_become_one_gray_filler() {
        let t = flat(&[1000, 1, 1, 1]);
        let out = layout(&t, 0, 400, 100, &p(0, 0));
        // the three tiny files share a region narrower than 32 px: one filler
        assert!(out.iter().any(|b| b.node.is_none()));
        assert_eq!(out.iter().filter(|b| b.node.is_some()).count(), 1);
    }

    #[test]
    fn folder_children_are_inset() {
        let mut nodes = vec![node("/", 100, None, -1, true), node("d", 100, Some(0), 0, true), node("a", 100, Some(1), 1, false)];
        nodes[0].children = vec![1];
        nodes[1].children = vec![2];
        let t = Tree { nodes, root_path: PathBuf::from("/"), fs_total: 0, fs_free: 0, free_node: None };
        let out = layout(&t, 0, 200, 100, &p(0, 0));
        assert_eq!(out[1].rect, IRect { x: 3, y: 12, w: 194, h: 85 });
        assert_eq!(out[1].depth, 1);
    }
}
