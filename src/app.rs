//! The main window: toolbar, space map, status bar and dialogs.

use crate::layout::{self, IRect, Placed};
use crate::scan::{self, Progress, Tree};
use crate::settings::{self, Settings, COLOR_LABELS, DENSITY_LABELS};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Margin, Pos2, Rect, Sense, Stroke, StrokeKind,
    Vec2,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// Classic Windows 3D colours (button face, highlight, shadow).
const FACE: Color32 = Color32::from_rgb(192, 192, 192);
const HILITE: Color32 = Color32::from_rgb(255, 255, 255);
const SHADOW: Color32 = Color32::from_rgb(128, 128, 128);
const DKSHADOW: Color32 = Color32::from_rgb(64, 64, 64);
const BLACK: Color32 = Color32::BLACK;

// Rainbow tiers, tables 0x42c0c0 / 0x42c0e0 / 0x42c100.
const RAINBOW_FILL: [(u8, u8, u8); 8] = [
    (255, 127, 127),
    (255, 191, 127),
    (255, 255, 0),
    (127, 255, 127),
    (127, 255, 255),
    (191, 191, 255),
    (191, 191, 191),
    (255, 127, 255),
];
const RAINBOW_LIGHT: [(u8, u8, u8); 8] = [
    (255, 191, 191),
    (255, 223, 191),
    (255, 255, 191),
    (191, 255, 191),
    (223, 255, 255),
    (223, 223, 255),
    (223, 223, 223),
    (255, 191, 255),
];
const RAINBOW_DARK: [(u8, u8, u8); 8] = [
    (191, 127, 127),
    (191, 159, 95),
    (191, 191, 63),
    (127, 191, 127),
    (127, 191, 191),
    (159, 159, 255),
    (159, 159, 159),
    (191, 127, 191),
];
// Solid choices 2..=11 (tables 0x42c128 / 0x42c150 / 0x42c178).
const SOLID_FILL: [(u8, u8, u8); 10] = [
    (255, 255, 255),
    (191, 191, 191),
    (127, 127, 127),
    (255, 127, 127),
    (255, 191, 127),
    (255, 255, 0),
    (127, 255, 127),
    (127, 255, 255),
    (191, 191, 255),
    (255, 127, 255),
];
const SOLID_LIGHT: [(u8, u8, u8); 10] = [
    (255, 255, 255),
    (255, 255, 255),
    (191, 191, 191),
    (255, 159, 159),
    (255, 223, 191),
    (255, 255, 191),
    (191, 255, 191),
    (223, 255, 255),
    (223, 223, 255),
    (255, 191, 255),
];
const SOLID_DARK: [(u8, u8, u8); 10] = [
    (191, 191, 191),
    (127, 127, 127),
    (63, 63, 63),
    (191, 127, 127),
    (191, 159, 159),
    (191, 191, 63),
    (127, 191, 127),
    (127, 191, 191),
    (159, 159, 255),
    (191, 127, 191),
];

fn rgb(c: (u8, u8, u8)) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

/// (fill, light, dark) for a colour choice and depth.
fn scheme(choice: i32, depth: i32) -> (Color32, Color32, Color32) {
    match choice {
        0 => {
            let i = depth.rem_euclid(8) as usize;
            (rgb(RAINBOW_FILL[i]), rgb(RAINBOW_LIGHT[i]), rgb(RAINBOW_DARK[i]))
        }
        1 => (FACE, HILITE, SHADOW),
        c => {
            let i = (c.clamp(2, 11) - 2) as usize;
            (rgb(SOLID_FILL[i]), rgb(SOLID_LIGHT[i]), rgb(SOLID_DARK[i]))
        }
    }
}

const LABEL_SIZE: f32 = 10.0;
/// Zoom animation: 8 outline steps, 25 ms apart (0x406110: loop to 8, Sleep(0x19)).
const ANIM_STEPS: i32 = 8;
const ANIM_STEP_SECS: f32 = 0.025;

#[derive(Clone, Copy, PartialEq)]
enum Cmd {
    Open,
    Reload,
    ZoomFull,
    ZoomIn,
    ZoomOut,
    FreeSpace,
    Run,
    Delete,
    Setup,
    About,
}

const TOOLBAR: [(Cmd, &str, &str); 10] = [
    (Cmd::Open, "Open", ""),
    (Cmd::Reload, "Reload", ""),
    (Cmd::ZoomFull, "Zoom", "Full"),
    (Cmd::ZoomIn, "Zoom", "In"),
    (Cmd::ZoomOut, "Zoom", "Out"),
    (Cmd::FreeSpace, "Free", "Space"),
    (Cmd::Run, "Run or", "Open"),
    (Cmd::Delete, "Delete", ""),
    (Cmd::Setup, "Setup", ""),
    (Cmd::About, "About", ""),
];

struct ScanJob {
    path: PathBuf,
    progress: Arc<Progress>,
    handle: Option<JoinHandle<Option<Tree>>>,
    restore_view: Option<PathBuf>,
}

/// The original's zoom animation (0x406110): an inverted outline rectangle stepped
/// 8 times, 25 ms apart, over the old picture; then the new view is drawn.
struct Anim {
    from: Rect,
    to: Rect,
    start: Instant,
    old: Vec<Placed>,
}

struct Shot {
    path: PathBuf,
    frames: u32,
    requested: bool,
    done: Arc<AtomicBool>,
    started: Instant,
}

struct Mount {
    path: PathBuf,
    label: String,
    fstype: String,
    total: u64,
    free: u64,
}

type LayoutKey = (i32, i32, usize, i32, i32, bool, u64);

pub struct SpaceApp {
    settings: Settings,
    tree: Option<Tree>,
    view_root: usize,
    selected: Option<usize>,
    show_free: bool,
    placed: Vec<Placed>,
    layout_key: Option<LayoutKey>,
    tree_gen: u64,
    scan: Option<ScanJob>,
    anim: Option<Anim>,
    hover: Option<(usize, Instant)>,
    drive_dialog: bool,
    drive_text: String,
    drive_pick: Option<usize>,
    mounts: Vec<Mount>,
    settings_dialog: Option<Settings>,
    about: bool,
    confirm_delete: Option<usize>,
    properties: Option<usize>,
    message: Option<String>,
    shot: Option<Shot>,
    last_window: Option<[f32; 4]>,
}

impl SpaceApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        settings: Settings,
        folder: Option<PathBuf>,
        screenshot: Option<PathBuf>,
    ) -> Self {
        install_fonts(&cc.egui_ctx);
        cc.egui_ctx.set_visuals(classic_visuals());
        cc.egui_ctx.all_styles_mut(|style| {
            style.text_styles.insert(egui::TextStyle::Body, FontId::proportional(12.0));
            style.text_styles.insert(egui::TextStyle::Button, FontId::proportional(12.0));
            style.text_styles.insert(egui::TextStyle::Small, FontId::proportional(10.0));
            style.spacing.item_spacing = Vec2::new(6.0, 5.0);
            style.spacing.button_padding = Vec2::new(8.0, 3.0);
        });

        let mut app = SpaceApp {
            settings,
            tree: None,
            view_root: 0,
            selected: None,
            show_free: false,
            placed: Vec::new(),
            layout_key: None,
            tree_gen: 0,
            scan: None,
            anim: None,
            hover: None,
            drive_dialog: false,
            drive_text: String::new(),
            drive_pick: None,
            mounts: Vec::new(),
            settings_dialog: None,
            about: false,
            confirm_delete: None,
            properties: None,
            message: None,
            shot: screenshot.map(|path| Shot {
                path,
                frames: 0,
                requested: false,
                done: Arc::new(AtomicBool::new(false)),
                started: Instant::now(),
            }),
            last_window: None,
        };
        // For the automatic check picture: a stable, non-personal folder.
        let folder = folder.or_else(|| app.shot.as_ref().map(|_| PathBuf::from("/usr")));
        match folder {
            Some(f) => app.start_scan(f, None),
            None => app.open_drive_dialog(),
        }
        app
    }

    // ---------- scanning ----------

    fn start_scan(&mut self, path: PathBuf, restore_view: Option<PathBuf>) {
        if let Some(job) = &self.scan {
            job.progress.cancel.store(true, Ordering::Relaxed);
        }
        let progress = Arc::new(Progress::default());
        let p2 = progress.clone();
        let path2 = path.clone();
        let handle = std::thread::Builder::new()
            .name("scan".into())
            .stack_size(256 * 1024 * 1024)
            .spawn(move || scan::scan(path2, p2))
            .ok();
        self.scan = Some(ScanJob { path, progress, handle, restore_view });
    }

    fn poll_scan(&mut self, ctx: &egui::Context) {
        let finished = self.scan.as_ref().and_then(|j| j.handle.as_ref()).map(|h| h.is_finished());
        match finished {
            None => {
                if self.scan.as_ref().is_some_and(|j| j.handle.is_none()) {
                    self.scan = None;
                    self.message = Some("Could not start scanning.".into());
                }
                return;
            }
            Some(false) => {
                ctx.request_repaint_after(Duration::from_millis(50));
                return;
            }
            Some(true) => {}
        }
        let mut job = self.scan.take().unwrap();
        let result = job.handle.take().and_then(|h| h.join().ok()).flatten();
        match result {
            Some(tree) => {
                let mut view = tree.root();
                if let Some(rp) = &job.restore_view {
                    view = find_path(&tree, rp).unwrap_or(view);
                }
                self.tree = Some(tree);
                self.view_root = view;
                self.selected = None;
                self.anim = None;
                self.tree_gen += 1;
                self.layout_key = None;
            }
            None => {
                if !job.progress.cancel.load(Ordering::Relaxed) {
                    self.message = Some(format!("Could not read the folder {}", job.path.display()));
                }
            }
        }
        if let Some(s) = &mut self.shot {
            s.frames = 0;
        }
    }

    // ---------- commands ----------

    fn can(&self, cmd: Cmd) -> bool {
        let Some(t) = &self.tree else {
            return matches!(cmd, Cmd::Open | Cmd::Setup | Cmd::About);
        };
        match cmd {
            Cmd::Open | Cmd::Setup | Cmd::About | Cmd::Reload | Cmd::FreeSpace => true,
            Cmd::ZoomFull => self.view_root != t.root(),
            Cmd::ZoomIn => self.zoom_in_target().is_some(),
            Cmd::ZoomOut => t.nodes[self.view_root].parent.is_some(),
            Cmd::Run => self.selected.is_some_and(|s| !t.nodes[s].is_free),
            Cmd::Delete => {
                !self.settings.disable_delete
                    && self.selected.is_some_and(|s| !t.nodes[s].is_free && t.nodes[s].parent.is_some())
            }
        }
    }

    fn zoom_in_target(&self) -> Option<usize> {
        let t = self.tree.as_ref()?;
        let s = self.selected?;
        let n = &t.nodes[s];
        if n.is_free {
            return None;
        }
        let target = if n.is_dir { s } else { n.parent? };
        (target != self.view_root && t.is_ancestor(self.view_root, target)).then_some(target)
    }

    fn run(&mut self, cmd: Cmd, ctx: &egui::Context) {
        if !self.can(cmd) {
            return;
        }
        match cmd {
            Cmd::Open => self.open_drive_dialog(),
            Cmd::Reload => self.rescan(),
            Cmd::ZoomFull => self.set_view(0, ctx),
            Cmd::ZoomIn => {
                if let Some(t) = self.zoom_in_target() {
                    self.set_view(t, ctx);
                }
            }
            Cmd::ZoomOut => {
                let p = self.tree.as_ref().and_then(|t| t.nodes[self.view_root].parent);
                if let Some(p) = p {
                    self.set_view(p, ctx);
                }
            }
            Cmd::FreeSpace => {
                self.show_free = !self.show_free;
                self.layout_key = None;
            }
            Cmd::Run => {
                if let (Some(t), Some(s)) = (&self.tree, self.selected) {
                    let p = t.path_of(s);
                    if let Err(e) = std::process::Command::new("xdg-open").arg(&p).spawn() {
                        self.message = Some(format!("Could not open {}:\n{}", p.display(), e));
                    }
                }
            }
            Cmd::Delete => self.confirm_delete = self.selected,
            Cmd::Setup => self.settings_dialog = Some(self.settings.clone()),
            Cmd::About => self.about = true,
        }
    }

    fn rescan(&mut self) {
        if let Some(t) = &self.tree {
            let root = t.root_path.clone();
            let view = t.path_of(self.view_root);
            self.start_scan(root, Some(view));
        }
    }

    /// Change the view root, animating like the original's "Animated Zoom".
    fn set_view(&mut self, new_root: usize, ctx: &egui::Context) {
        if new_root == self.view_root {
            return;
        }
        let Some(tree) = &self.tree else { return };
        let old = self.view_root;
        let map = self.map_size();
        let full = Rect::from_min_size(Pos2::ZERO, map);
        let mut fromto = None;
        if self.settings.animated_zoom && map.x > 4.0 && map.y > 4.0 {
            if tree.is_ancestor(old, new_root) {
                // zooming in: the outline grows from the folder's box to the whole map
                if let Some(p) = self.placed.iter().find(|p| p.node == Some(new_root)) {
                    fromto = Some((irect(p.rect), full));
                }
            } else if tree.is_ancestor(new_root, old) {
                // zooming out: the outline shrinks from the whole map to where the old view goes
                let new_layout = layout::layout(tree, new_root, map.x as i32, map.y as i32, &self.params());
                if let Some(p) = new_layout.iter().find(|p| p.node == Some(old)) {
                    fromto = Some((full, irect(p.rect)));
                }
            }
        }
        self.view_root = new_root;
        if !tree.is_ancestor(new_root, self.selected.unwrap_or(new_root)) {
            self.selected = None;
        }
        self.layout_key = None;
        let old_placed = self.placed.clone();
        self.anim = fromto.map(|(from, to)| Anim { from, to, start: Instant::now(), old: old_placed });
        ctx.request_repaint();
    }

    fn map_size(&self) -> Vec2 {
        match self.layout_key {
            Some((w, h, ..)) => Vec2::new(w as f32, h as f32),
            None => Vec2::ZERO,
        }
    }

    fn params(&self) -> layout::Params {
        layout::Params { density: self.settings.density, bias: self.settings.bias, show_free: self.show_free }
    }

    fn do_delete(&mut self, idx: usize) {
        let Some(t) = &mut self.tree else { return };
        let path = t.path_of(idx);
        let r = if t.nodes[idx].is_dir { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        match r {
            Ok(()) => {
                if t.is_ancestor(idx, self.view_root) {
                    self.view_root = t.nodes[idx].parent.unwrap_or(0);
                }
                t.remove(idx);
                self.selected = None;
                self.tree_gen += 1;
                self.layout_key = None;
            }
            Err(e) => self.message = Some(format!("Could not delete {}:\n{}", path.display(), e)),
        }
        if self.settings.auto_rescan {
            self.rescan();
        }
    }

    fn open_drive_dialog(&mut self) {
        self.mounts = list_mounts();
        self.drive_pick = None;
        if self.drive_text.is_empty() {
            self.drive_text = self
                .tree
                .as_ref()
                .map(|t| t.root_path.display().to_string())
                .or_else(|| std::env::var("HOME").ok())
                .unwrap_or_else(|| "/".into());
        }
        self.drive_dialog = true;
    }

    // ---------- drawing ----------

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 1.0;
            ui.add_space(2.0);
            for (i, (cmd, l1, l2)) in TOOLBAR.iter().enumerate() {
                if i == 2 || i == 5 || i == 8 {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(6.0, 36.0), Sense::hover());
                    let x = r.center().x.round() - 0.5;
                    ui.painter().line_segment([Pos2::new(x, r.top() + 3.0), Pos2::new(x, r.bottom() - 3.0)], (1.0, SHADOW));
                    ui.painter()
                        .line_segment([Pos2::new(x + 1.0, r.top() + 3.0), Pos2::new(x + 1.0, r.bottom() - 3.0)], (1.0, HILITE));
                }
                let enabled = self.can(*cmd);
                let toggled = *cmd == Cmd::FreeSpace && self.show_free;
                let (rect, resp) =
                    ui.allocate_exact_size(Vec2::new(66.0, 36.0), if enabled { Sense::click() } else { Sense::hover() });
                let p = ui.painter();
                let pressed = (enabled && resp.is_pointer_button_down_on()) || toggled;
                if pressed || (enabled && resp.hovered()) {
                    bevel(p, rect, !pressed);
                }
                let off = if pressed { Vec2::new(1.0, 1.0) } else { Vec2::ZERO };
                let icon = Rect::from_center_size(Pos2::new(rect.left() + 13.0, rect.center().y) + off, Vec2::splat(18.0));
                draw_icon(p, *cmd, icon, enabled);
                let color = if enabled { BLACK } else { SHADOW };
                let font = FontId::new(11.0, FontFamily::Name("bold".into()));
                let tx = rect.left() + 25.0 + off.x;
                let lines: Vec<(&str, f32)> =
                    if l2.is_empty() { vec![(*l1, 0.0)] } else { vec![(*l1, -6.5), (*l2, 6.5)] };
                for (l, dy) in lines {
                    let pos = Pos2::new(tx, rect.center().y + off.y + dy);
                    if !enabled {
                        p.text(pos + Vec2::new(1.0, 1.0), Align2::LEFT_CENTER, l, font.clone(), HILITE);
                    }
                    p.text(pos, Align2::LEFT_CENTER, l, font.clone(), color);
                }
                if resp.clicked() {
                    self.run(*cmd, &ctx);
                }
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let p = ui.painter();
        let h = rect.height() - 4.0;
        let font = FontId::proportional(12.0);
        let (left, right) = match &self.tree {
            None => {
                let s = if self.scan.is_some() { "Scanning Disk..." } else { "Ready" };
                (s.to_string(), String::new())
            }
            Some(t) => {
                let focus = self
                    .hover
                    .and_then(|(pi, _)| self.placed.get(pi))
                    .and_then(|p| p.node)
                    .or(self.selected);
                let left = match focus {
                    Some(i) if t.nodes[i].is_free => format!("Free Space: {}", scan::fmt_size(t.fs_free)),
                    Some(i) => format!("{}    {}", t.path_of(i).display(), scan::fmt_size(t.nodes[i].size)),
                    None => format!(
                        "{}    {}",
                        t.path_of(self.view_root).display(),
                        scan::fmt_size(t.nodes[self.view_root].size)
                    ),
                };
                let v = &t.nodes[self.view_root];
                let right = format!(
                    "Files Total: {}   Folders Total: {}   Free: {} ({})",
                    scan::fmt_count(v.files),
                    scan::fmt_count(v.dirs),
                    scan::fmt_size(t.fs_free),
                    scan::fmt_percent(t.fs_free, t.fs_total)
                );
                (left, right)
            }
        };
        let rg = p.layout_no_wrap(right.clone(), font.clone(), BLACK);
        let rw = if right.is_empty() { 0.0 } else { rg.size().x + 12.0 };
        let r_right = Rect::from_min_size(Pos2::new(rect.right() - rw - 2.0, rect.top() + 2.0), Vec2::new(rw, h));
        let r_left = Rect::from_min_max(
            Pos2::new(rect.left() + 2.0, rect.top() + 2.0),
            Pos2::new(r_right.left() - 3.0, rect.top() + 2.0 + h),
        );
        for r in [r_left, r_right] {
            if r.width() > 4.0 {
                bevel(p, r, false);
            }
        }
        p.with_clip_rect(r_left.shrink(2.0)).text(
            Pos2::new(r_left.left() + 5.0, r_left.center().y),
            Align2::LEFT_CENTER,
            left,
            font,
            BLACK,
        );
        if !right.is_empty() {
            p.galley(Pos2::new(r_right.left() + 6.0, r_right.center().y - rg.size().y / 2.0), rg, BLACK);
        }
    }

    fn map(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();
        let resp = ui.allocate_rect(rect, Sense::click());
        let painter = ui.painter_at(rect);
        let origin = rect.min;
        let (w, h) = (rect.width().floor() as i32, rect.height().floor() as i32);

        let Some(tree) = &self.tree else {
            painter.rect_filled(rect, 0.0, SHADOW);
            if self.scan.is_none() {
                painter.text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    "Click Open to choose a drive or folder",
                    FontId::proportional(14.0),
                    Color32::WHITE,
                );
            }
            return;
        };

        let key = (w, h, self.view_root, self.settings.density, self.settings.bias, self.show_free, self.tree_gen);
        if self.layout_key != Some(key) {
            self.placed = layout::layout(tree, self.view_root, w, h, &self.params());
            self.layout_key = Some(key);
            self.hover = None;
        }

        painter.rect_filled(rect, 0.0, FACE);

        let mut anim_t = None;
        if let Some(a) = &self.anim {
            // 8 steps of 25 ms
            let step = (a.start.elapsed().as_secs_f32() / ANIM_STEP_SECS).floor() as i32 + 1;
            if step > ANIM_STEPS {
                self.anim = None;
            } else {
                anim_t = Some(step as f32 / ANIM_STEPS as f32);
                ctx.request_repaint_after(Duration::from_millis(5));
            }
        }
        let xf = |r: Rect| -> Rect { r.translate(origin.to_vec2()) };

        let pointer = resp.hover_pos();
        let hovered_pi = if anim_t.is_none() {
            pointer.and_then(|pos| {
                let lp = pos - origin.to_vec2();
                self.placed.iter().rposition(|p| p.rect.contains(lp.x, lp.y))
            })
        } else {
            None
        };
        match (self.hover, hovered_pi) {
            (Some((a, _)), Some(b)) if a == b => {}
            (_, Some(b)) => self.hover = Some((b, Instant::now())),
            (_, None) => self.hover = None,
        }

        let rollover_pi = if self.settings.show_rollover_box { hovered_pi } else { None };
        let shown: &Vec<Placed> = match (&self.anim, anim_t) {
            (Some(a), Some(_)) => &a.old,
            _ => &self.placed,
        };
        for (pi, pl) in shown.iter().enumerate() {
            let r = xf(irect(pl.rect));
            if !r.intersects(rect) {
                continue;
            }
            let node = pl.node.map(|i| &tree.nodes[i]);
            let selected = pl.node.is_some() && pl.node == self.selected;
            draw_box(&painter, r, pl, node, tree, &self.settings, selected, rollover_pi == Some(pi), true);
        }
        if let (Some(a), Some(t)) = (&self.anim, anim_t) {
            // inverted outline (R2_NOT in the original): black and white lines read on any colour
            let o = lerp_rect(a.from, a.to, t).translate(origin.to_vec2());
            painter.rect_stroke(o, 0.0, Stroke::new(1.0, Color32::WHITE), StrokeKind::Inside);
            painter.rect_stroke(o.shrink(1.0), 0.0, Stroke::new(1.0, BLACK), StrokeKind::Inside);
            painter.rect_stroke(o.expand(1.0), 0.0, Stroke::new(1.0, BLACK), StrokeKind::Inside);
        }

        let hovered_node = hovered_pi.and_then(|pi| self.placed[pi].node);
        let is_dir = hovered_node.map(|n| tree.nodes[n].is_dir && !tree.nodes[n].is_free);
        let is_free = hovered_node.is_some_and(|n| tree.nodes[n].is_free);
        if resp.clicked() || resp.secondary_clicked() {
            self.selected = hovered_node;
        }
        if resp.double_clicked() {
            if let Some(n) = hovered_node {
                self.selected = Some(n);
                if is_dir == Some(true) {
                    self.run(Cmd::ZoomIn, &ctx);
                } else if !is_free {
                    self.run(Cmd::Run, &ctx);
                }
            }
        }
        if ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Extra1)) {
            self.run(Cmd::ZoomOut, &ctx);
        }

        resp.context_menu(|ui| {
            ui.set_min_width(170.0);
            let items: [(Cmd, &str); 5] = [
                (Cmd::ZoomIn, "Zoom In"),
                (Cmd::ZoomOut, "Zoom Out"),
                (Cmd::ZoomFull, "Zoom Full"),
                (Cmd::Run, "Run / Open"),
                (Cmd::Delete, "Delete"),
            ];
            for (cmd, label) in items {
                if ui.add_enabled(self.can(cmd), egui::Button::new(label)).clicked() {
                    self.run(cmd, &ctx);
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Open Drive...").clicked() {
                self.run(Cmd::Open, &ctx);
                ui.close();
            }
            if ui.button("Rescan Drive").clicked() {
                self.run(Cmd::Reload, &ctx);
                ui.close();
            }
            let mut sf = self.show_free;
            if ui.checkbox(&mut sf, "Show Free Space").clicked() {
                self.run(Cmd::FreeSpace, &ctx);
                ui.close();
            }
            ui.separator();
            if ui.add_enabled(self.selected.is_some(), egui::Button::new("Properties...")).clicked() {
                self.properties = self.selected;
                ui.close();
            }
        });

        // name and info tips
        if anim_t.is_some() || egui::Popup::is_any_open(&ctx) {
            return;
        }
        let (Some((pi, since)), Some(pos)) = (self.hover, pointer) else { return };
        let Some(tree) = &self.tree else { return };
        let pl = &self.placed[pi];
        let Some(ni) = pl.node else { return };
        let el = since.elapsed().as_millis() as u32;
        let name_on = self.settings.show_name_tips && el >= self.settings.nametip_delay;
        let info_on = self.settings.show_info_tips && el >= self.settings.infotip_delay;
        let need = [
            (self.settings.show_name_tips, self.settings.nametip_delay),
            (self.settings.show_info_tips, self.settings.infotip_delay),
        ]
        .iter()
        .filter(|(on, d)| *on && *d > el)
        .map(|(_, d)| *d)
        .min();
        if let Some(d) = need {
            ctx.request_repaint_after(Duration::from_millis((d - el) as u64 + 5));
        }
        if !(name_on || info_on) {
            return;
        }
        let lines = tip_lines(tree, ni, &self.settings, info_on);
        let n = &tree.nodes[ni];
        let (fill, light, _) = colors_for(pl, n, &self.settings);
        // the tip takes the box's colour; the selected box's tip is black with white text (0x404102)
        let (bg, fg) = if self.selected == Some(ni) {
            (BLACK, Color32::WHITE)
        } else if n.is_free {
            (FACE, BLACK)
        } else if pl.is_folder {
            (fill, BLACK)
        } else {
            (light, BLACK)
        };
        let screen = ctx.content_rect();
        let mut tip_pos = pos + Vec2::new(12.0, 18.0);
        let est_w = lines.iter().map(|l| l.chars().count() as f32 * 6.5).fold(60.0, f32::max) + 10.0;
        if tip_pos.x + est_w > screen.right() {
            tip_pos.x = (screen.right() - est_w).max(screen.left());
        }
        let est_h = lines.len() as f32 * 15.0 + 6.0;
        if tip_pos.y + est_h > screen.bottom() {
            tip_pos.y = pos.y - est_h - 4.0;
        }
        egui::Area::new(egui::Id::unique("box_tip"))
            .order(egui::Order::Tooltip)
            .fixed_pos(tip_pos)
            .interactable(false)
            .show(&ctx, |ui| {
                Frame::NONE.fill(bg).stroke(Stroke::new(1.0, BLACK)).inner_margin(Margin::symmetric(4, 2)).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for (k, l) in lines.iter().enumerate() {
                        let f = if k == 0 {
                            FontId::new(11.0, FontFamily::Name("bold".into()))
                        } else {
                            FontId::proportional(11.0)
                        };
                        ui.add(egui::Label::new(egui::RichText::new(l).font(f).color(fg)).extend());
                    }
                });
            });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.scan {
            let folders = job.progress.folders.load(Ordering::Relaxed);
            let files = job.progress.files.load(Ordering::Relaxed);
            let current = job.progress.current.lock().map(|s| s.clone()).unwrap_or_default();
            let path = job.path.display().to_string();
            let mut cancel = false;
            dialog(ctx, "Scanning Disk...", |ui| {
                ui.set_width(380.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(48.0, 40.0), Sense::hover());
                    scan_anim(ui.painter(), r, ui.input(|i| i.time));
                    ui.vertical(|ui| {
                        ui.label(format!("Scanning {}", path));
                        egui::Grid::new("scan_counts").num_columns(2).show(ui, |ui| {
                            ui.label("Folders Found:");
                            ui.label(scan::fmt_count(folders));
                            ui.end_row();
                            ui.label("Files Found:");
                            ui.label(scan::fmt_count(files));
                            ui.end_row();
                        });
                    });
                });
                ui.add(egui::Label::new(egui::RichText::new(shorten(&current, 60)).small().color(DKSHADOW)).truncate());
                ui.vertical_centered(|ui| {
                    if ui.add(egui::Button::new("Cancel").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        cancel = true;
                    }
                });
            });
            if cancel {
                job.progress.cancel.store(true, Ordering::Relaxed);
            }
            ctx.request_repaint_after(Duration::from_millis(60));
        }

        if self.drive_dialog && self.scan.is_none() {
            let mut ok = false;
            let mut cancel = false;
            dialog(ctx, "Select Drive to View", |ui| {
                ui.set_width(470.0);
                ui.label("Choose a drive, or type the folder to map:");
                Frame::NONE.fill(Color32::WHITE).stroke(Stroke::new(1.0, SHADOW)).inner_margin(Margin::same(2)).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(190.0).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 0.0;
                        for (i, m) in self.mounts.iter().enumerate() {
                            let sel = self.drive_pick == Some(i);
                            let text = if m.total > 0 {
                                format!(
                                    "{}   [{}]   {} free of {}",
                                    m.label,
                                    m.fstype,
                                    scan::fmt_size(m.free),
                                    scan::fmt_size(m.total)
                                )
                            } else {
                                m.label.clone()
                            };
                            let r = ui.add(
                                egui::Button::selectable(sel, text).min_size(Vec2::new(ui.available_width(), 20.0)),
                            );
                            if r.clicked() {
                                self.drive_pick = Some(i);
                                self.drive_text = m.path.display().to_string();
                            }
                            if r.double_clicked() {
                                self.drive_text = m.path.display().to_string();
                                ok = true;
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Folder:");
                    let r = ui.add(egui::TextEdit::singleline(&mut self.drive_text).desired_width(400.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        ok = true;
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 172.0).max(0.0));
                    if ui.add(egui::Button::new("OK").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        ok = true;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        cancel = true;
                    }
                });
            });
            if ok {
                let p = PathBuf::from(expand_home(self.drive_text.trim()));
                if p.is_dir() {
                    self.drive_dialog = false;
                    self.start_scan(p, None);
                } else {
                    self.message = Some(format!("{} is not a folder.", p.display()));
                }
            } else if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.drive_dialog = false;
            }
        }

        if let Some(mut s) = self.settings_dialog.take() {
            let mut keep = true;
            let mut apply = false;
            dialog(ctx, "Settings", |ui| {
                ui.set_width(440.0);
                group(ui, "File Layout", |ui| {
                    egui::Grid::new("layout_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        ui.label("Density:");
                        let mut idx = (s.density + 3) as usize;
                        egui::ComboBox::from_id_salt("density").width(180.0).selected_text(DENSITY_LABELS[idx]).show_ui(
                            ui,
                            |ui| {
                                for (i, l) in DENSITY_LABELS.iter().enumerate() {
                                    ui.selectable_value(&mut idx, i, *l);
                                }
                            },
                        );
                        s.density = idx as i32 - 3;
                        ui.end_row();
                        ui.label("Bias:");
                        ui.vertical(|ui| {
                            ui.spacing_mut().slider_width = 240.0;
                            ui.add(egui::Slider::new(&mut s.bias, -20..=20).show_value(false));
                            let (r, _) = ui.allocate_exact_size(Vec2::new(240.0, 14.0), Sense::hover());
                            let f = FontId::proportional(11.0);
                            ui.painter().text(r.left_center(), Align2::LEFT_CENTER, "Horz", f.clone(), BLACK);
                            ui.painter().text(r.center(), Align2::CENTER_CENTER, "Equal", f.clone(), BLACK);
                            ui.painter().text(r.right_center(), Align2::RIGHT_CENTER, "Vert", f, BLACK);
                        });
                        ui.end_row();
                    });
                });
                group(ui, "Display Colors", |ui| {
                    egui::Grid::new("color_grid").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                        for (label, val, id) in [("Folders", &mut s.folder_color, "fc"), ("Files", &mut s.file_color, "flc")] {
                            ui.label(label);
                            let mut idx = *val as usize;
                            egui::ComboBox::from_id_salt(id).width(180.0).selected_text(COLOR_LABELS[idx]).show_ui(ui, |ui| {
                                for (i, l) in COLOR_LABELS.iter().enumerate() {
                                    ui.selectable_value(&mut idx, i, *l);
                                }
                            });
                            *val = idx as i32;
                            ui.end_row();
                        }
                    });
                });
                group(ui, "ToolTips", |ui| {
                    for (on, delay, label) in [
                        (&mut s.show_name_tips, &mut s.nametip_delay, "Show file-name-tips"),
                        (&mut s.show_info_tips, &mut s.infotip_delay, "Show file-info-tips"),
                    ] {
                        ui.horizontal(|ui| {
                            ui.checkbox(on, label);
                            ui.add_space((ui.available_width() - 130.0).max(0.0));
                            ui.label("Delay:");
                            ui.add(egui::DragValue::new(delay).range(0..=5000).speed(5));
                            ui.label("msec");
                        });
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(20.0);
                        for (bit, label) in [
                            (settings::TIP_FULL_PATH, "Full Path"),
                            (settings::TIP_FILENAME, "Filename"),
                            (settings::TIP_DATE, "Date / Time"),
                            (settings::TIP_SIZE, "File Size"),
                            (settings::TIP_ATTRS, "Attributes"),
                        ] {
                            let mut on = s.infotip_flags & bit != 0;
                            if ui.checkbox(&mut on, label).changed() {
                                if on {
                                    s.infotip_flags |= bit;
                                } else {
                                    s.infotip_flags &= !bit;
                                }
                            }
                        }
                    });
                });
                group(ui, "Miscellaneous Options", |ui| {
                    egui::Grid::new("misc").num_columns(2).show(ui, |ui| {
                        ui.checkbox(&mut s.show_rollover_box, "Show Rollover Boxes");
                        ui.checkbox(&mut s.save_pos, "Remember Window Position");
                        ui.end_row();
                        ui.checkbox(&mut s.animated_zoom, "Animated Zoom In / Zoom Out");
                        ui.checkbox(&mut s.disable_delete, "Disable \"Delete\" Command");
                        ui.end_row();
                        ui.checkbox(&mut s.auto_rescan, "Auto Rescan on Delete");
                        ui.end_row();
                    });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("Defaults").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        let w = s.window;
                        s = Settings::default();
                        s.window = w;
                    }
                    ui.add_space((ui.available_width() - 172.0).max(0.0));
                    if ui.add(egui::Button::new("OK").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        apply = true;
                        keep = false;
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        keep = false;
                    }
                });
            });
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                keep = false;
            }
            if apply {
                s.window = self.settings.window;
                self.settings = s.clone();
                self.settings.save();
                self.layout_key = None;
            }
            if keep {
                self.settings_dialog = Some(s);
            }
        }

        if self.about {
            let mut close = false;
            dialog(ctx, "About kSpaceMonger", |ui| {
                ui.set_width(360.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                    draw_icon(ui.painter(), Cmd::About, r, true);
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("kSpaceMonger 0.1.0").font(FontId::new(16.0, FontFamily::Name("bold".into()))),
                        );
                        ui.label("A fast disk space map for Linux,");
                        ui.label("in the style of the classic 1990s space mappers.");
                    });
                });
                ui.add_space(6.0);
                ui.label("Bigger boxes use more space. Click a box to select it,");
                ui.label("double-click a folder to zoom in, right-click for more.");
                ui.add_space(6.0);
                ui.vertical_centered(|ui| {
                    if ui.add(egui::Button::new("OK").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        close = true;
                    }
                });
            });
            if close || ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) {
                self.about = false;
            }
        }

        if let Some(idx) = self.confirm_delete {
            let (path, size, is_dir) = match &self.tree {
                Some(t) => (t.path_of(idx), t.nodes[idx].size, t.nodes[idx].is_dir),
                None => (PathBuf::new(), 0, false),
            };
            let mut answer = None;
            dialog(ctx, "Delete", |ui| {
                ui.set_width(400.0);
                ui.label(if is_dir {
                    "Delete this folder and everything in it?"
                } else {
                    "Delete this file?"
                });
                ui.label(egui::RichText::new(path.display().to_string()).strong());
                ui.label(format!("Size: {}", scan::fmt_size(size)));
                ui.label("This can't be undone.");
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 172.0).max(0.0));
                    if ui.add(egui::Button::new("Delete").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        answer = Some(true);
                    }
                    if ui.add(egui::Button::new("Cancel").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        answer = Some(false);
                    }
                });
            });
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                answer = Some(false);
            }
            if let Some(a) = answer {
                self.confirm_delete = None;
                if a {
                    self.do_delete(idx);
                }
            }
        }

        if let (Some(idx), Some(t)) = (self.properties, &self.tree) {
            let n = &t.nodes[idx];
            let mut close = false;
            let rows: Vec<(&str, String)> = if n.is_free {
                vec![
                    ("Free", format!("{}  ({})", scan::fmt_size(t.fs_free), scan::fmt_percent(t.fs_free, t.fs_total))),
                    ("Total", scan::fmt_size(t.fs_total)),
                ]
            } else {
                let mut v = vec![
                    ("Name", n.name.clone()),
                    ("Location", t.path_of(idx).parent().map(|p| p.display().to_string()).unwrap_or_default()),
                    ("Size", format!("{}  ({} bytes)", scan::fmt_size(n.size), scan::fmt_count(n.size))),
                    ("Share of view", scan::fmt_percent(n.size, t.nodes[self.view_root].size)),
                    ("Date / Time", scan::fmt_time(n.mtime)),
                    ("Attributes", scan::fmt_attrs(n)),
                ];
                if n.is_dir {
                    v.push(("Files Total:", scan::fmt_count(n.files)));
                    v.push(("Folders Total:", scan::fmt_count(n.dirs)));
                }
                v
            };
            dialog(ctx, "Properties", |ui| {
                ui.set_width(420.0);
                egui::Grid::new("props").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    for (k, val) in rows {
                        ui.label(k);
                        ui.label(val);
                        ui.end_row();
                    }
                });
                ui.vertical_centered(|ui| {
                    if ui.add(egui::Button::new("OK").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        close = true;
                    }
                });
            });
            if close || ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) {
                self.properties = None;
            }
        }

        if let Some(m) = self.message.clone() {
            let mut close = false;
            dialog(ctx, "kSpaceMonger", |ui| {
                ui.set_width(360.0);
                ui.label(m);
                ui.vertical_centered(|ui| {
                    if ui.add(egui::Button::new("OK").min_size(Vec2::new(80.0, 22.0))).clicked() {
                        close = true;
                    }
                });
            });
            if close || ctx.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)) {
                self.message = None;
            }
        }
    }

    fn any_dialog(&self) -> bool {
        self.scan.is_some()
            || self.drive_dialog
            || self.settings_dialog.is_some()
            || self.about
            || self.confirm_delete.is_some()
            || self.properties.is_some()
            || self.message.is_some()
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if self.any_dialog() || ctx.egui_wants_keyboard_input() {
            return;
        }
        let keys: [(egui::Key, Cmd); 7] = [
            (egui::Key::Enter, Cmd::ZoomIn),
            (egui::Key::Backspace, Cmd::ZoomOut),
            (egui::Key::Home, Cmd::ZoomFull),
            (egui::Key::F5, Cmd::Reload),
            (egui::Key::Delete, Cmd::Delete),
            (egui::Key::O, Cmd::Open),
            (egui::Key::F, Cmd::FreeSpace),
        ];
        for (k, c) in keys {
            if ctx.input(|i| i.key_pressed(k)) {
                self.run(c, ctx);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.selected = None;
        }
    }

    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let busy = self.scan.is_some();
        let Some(s) = &mut self.shot else { return };
        if s.done.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.request_repaint();
        if busy && s.started.elapsed() < Duration::from_secs(60) {
            return;
        }
        s.frames += 1;
        if s.frames >= 120 && !s.requested {
            s.requested = true;
            let path = s.path.clone();
            let done = s.done.clone();
            let c2 = ctx.clone();
            ctx.request_screenshot(move |img| {
                if let Err(e) = save_png(&path, &img) {
                    eprintln!("screenshot failed: {e}");
                }
                done.store(true, Ordering::Relaxed);
                c2.request_repaint();
            });
        }
    }
}

impl eframe::App for SpaceApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_scan(&ctx);
        self.keys(&ctx);

        let title = match &self.tree {
            Some(t) => format!("kSpaceMonger - {}", t.path_of(self.view_root).display()),
            None => "kSpaceMonger".to_string(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        let vp = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
        if let (Some(o), Some(i)) = vp {
            self.last_window = Some([o.min.x, o.min.y, i.width(), i.height()]);
        }

        egui::Panel::top("toolbar")
            .exact_size(42.0)
            .frame(Frame::NONE.fill(FACE).inner_margin(Margin::symmetric(0, 3)))
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status")
            .exact_size(22.0)
            .frame(Frame::NONE.fill(FACE))
            .show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().frame(Frame::NONE.fill(FACE).inner_margin(Margin::same(2))).show(ui, |ui| {
            let inner = ui.max_rect();
            bevel(ui.painter(), inner.expand(2.0), false);
            ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| self.map(ui));
        });

        self.dialogs(&ctx);
        self.screenshot_step(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if self.settings.save_pos && self.shot.is_none() {
            if let Some(w) = self.last_window {
                self.settings.window = Some(w);
                self.settings.save();
            }
        }
    }
}

// ---------- helpers ----------

fn classic_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    v.panel_fill = FACE;
    v.window_fill = FACE;
    v.faint_bg_color = Color32::from_rgb(212, 212, 212);
    v.extreme_bg_color = Color32::WHITE;
    v.window_corner_radius = CornerRadius::ZERO;
    v.menu_corner_radius = CornerRadius::ZERO;
    v.window_stroke = Stroke::new(1.0, DKSHADOW);
    v.override_text_color = Some(BLACK);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::ZERO;
    }
    v.widgets.noninteractive.corner_radius = CornerRadius::ZERO;
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(208, 208, 208);
    v.widgets.inactive.bg_fill = Color32::from_rgb(208, 208, 208);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, SHADOW);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(224, 224, 224);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, DKSHADOW);
    v.selection.bg_fill = Color32::from_rgb(0, 0, 128);
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v
}

fn irect(r: IRect) -> Rect {
    Rect::from_min_size(Pos2::new(r.x as f32, r.y as f32), Vec2::new(r.w as f32, r.h as f32))
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::from_min_max(a.min.lerp(b.min, t), a.max.lerp(b.max, t))
}

fn shorten(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        s.to_string()
    } else {
        let tail: String = s.chars().skip(n - (max - 3)).collect();
        format!("...{}", tail)
    }
}

fn colors_for(pl: &Placed, n: &scan::Node, s: &Settings) -> (Color32, Color32, Color32) {
    if n.is_free {
        return (FACE, FACE, FACE);
    }
    let choice = if pl.is_folder { s.folder_color } else { s.file_color };
    scheme(choice, pl.depth)
}

/// Raised (true) or sunken (false) Windows-style 1px bevel.
fn bevel(p: &egui::Painter, r: Rect, raised: bool) {
    let (tl, br) = if raised { (HILITE, SHADOW) } else { (SHADOW, HILITE) };
    p.rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width(), 1.0)), 0.0, tl);
    p.rect_filled(Rect::from_min_size(r.min, Vec2::new(1.0, r.height())), 0.0, tl);
    p.rect_filled(Rect::from_min_size(Pos2::new(r.min.x, r.max.y - 1.0), Vec2::new(r.width(), 1.0)), 0.0, br);
    p.rect_filled(Rect::from_min_size(Pos2::new(r.max.x - 1.0, r.min.y), Vec2::new(1.0, r.height())), 0.0, br);
}

#[allow(clippy::too_many_arguments)]
fn draw_box(
    p: &egui::Painter,
    r: Rect,
    pl: &Placed,
    node: Option<&scan::Node>,
    tree: &Tree,
    s: &Settings,
    selected: bool,
    rollover: bool,
    labels: bool,
) {
    if r.width() <= 2.0 || r.height() <= 2.0 {
        return;
    }
    let (mut fill, mut light, mut dark) = match node {
        Some(n) => colors_for(pl, n, s),
        None => (FACE, FACE, FACE),
    };
    if rollover && node.is_some_and(|n| !n.is_free) {
        dark = fill;
        fill = light;
        light = Color32::WHITE;
    }
    // black outline, light top-left / dark bottom-right bevel, then the fill
    p.rect_filled(r, 0.0, BLACK);
    let i1 = r.shrink(1.0);
    p.rect_filled(i1, 0.0, dark);
    p.rect_filled(Rect::from_min_max(i1.min, Pos2::new(i1.max.x - 1.0, i1.max.y - 1.0)), 0.0, light);
    p.rect_filled(r.shrink(2.0), 0.0, fill);

    let Some(n) = node else { return };
    if !labels {
        return;
    }
    let font = FontId::proportional(LABEL_SIZE);
    let (x, y, w, h) = (r.min.x, r.min.y, r.width(), r.height());
    let inner = r.shrink(2.0);
    let tc = if selected { Color32::WHITE } else { BLACK };

    if pl.is_folder && pl.opened {
        // header strip (x+3, y+3)..(x+w-3, y+12) holds the name, left at x+2, top at y+1
        let header = Rect::from_min_max(Pos2::new(x + 2.0, y + 2.0), Pos2::new(x + w - 2.0, y + layout::INSET_TOP as f32));
        if header.width() < 2.0 || header.height() < 2.0 {
            return;
        }
        let full = p.layout_no_wrap(format!("{}  {}", n.name, scan::fmt_size(n.size)), font.clone(), tc);
        let g = if full.size().x > header.width() - 2.0 { p.layout_no_wrap(n.name.clone(), font, tc) } else { full };
        let clip = p.with_clip_rect(header.expand2(Vec2::new(0.0, 1.0)).intersect(inner));
        if selected {
            clip.rect_filled(Rect::from_min_size(header.min, Vec2::new((g.size().x + 3.0).min(header.width()), header.height())), 0.0, BLACK);
        }
        let ty = y + 1.0 + (layout::INSET_TOP as f32 - 1.0 - g.size().y) / 2.0;
        clip.galley(Pos2::new(x + 3.0, ty), g, tc);
        return;
    }

    if n.is_free {
        // the free-space box carries a summary (0x405194): percent, free size, totals
        let root = &tree.nodes[tree.root()];
        let lines = [
            format!("<Free Space: {}>", scan::fmt_percent(tree.fs_free, tree.fs_total)),
            format!("{} Free", scan::fmt_size(tree.fs_free)),
            format!("Files Total:  {}", root.files),
            format!("Folders Total:  {}", root.dirs),
        ];
        let gs: Vec<_> = lines.iter().map(|l| p.layout_no_wrap(l.clone(), font.clone(), tc)).collect();
        let lh = gs[0].size().y;
        let fit = ((inner.height() / lh).floor() as usize).clamp(1, gs.len());
        let mut ty = if lh * fit as f32 <= inner.height() { y + (h - lh * fit as f32) / 2.0 } else { y + 1.0 };
        let clip = p.with_clip_rect(inner);
        for g in gs.into_iter().take(fit) {
            let tx = if g.size().x <= w - 4.0 { x + (w - g.size().x) / 2.0 } else { x + 2.0 };
            clip.galley(Pos2::new(tx, ty), g, tc);
            ty += lh;
        }
        return;
    }
    // files and closed folders: centred name, size under it when it fits
    let name = n.name.clone();
    let g = p.layout_no_wrap(name, font.clone(), tc);
    let size_line = w >= 48.0 && h >= 36.0;
    let gs = size_line.then(|| p.layout_no_wrap(scan::fmt_size(n.size), font, tc));
    let total_h = g.size().y + gs.as_ref().map(|g| g.size().y).unwrap_or(0.0);
    let ty = if total_h <= inner.height() { y + (h - total_h) / 2.0 } else { y + 1.0 };
    let tx = if g.size().x <= w - 4.0 { x + (w - g.size().x) / 2.0 } else { x + 2.0 };
    let clip = p.with_clip_rect(inner);
    if selected {
        let bw = g.size().x.max(gs.as_ref().map(|g| g.size().x).unwrap_or(0.0)) + 6.0;
        let bx = x + ((w - bw) / 2.0).max(2.0);
        clip.rect_filled(Rect::from_min_size(Pos2::new(bx, ty), Vec2::new(bw, total_h)), 0.0, BLACK);
    }
    let gh = g.size().y;
    clip.galley(Pos2::new(tx, ty), g, tc);
    if let Some(gs) = gs {
        let sx = if gs.size().x <= w - 4.0 { x + (w - gs.size().x) / 2.0 } else { x + 2.0 };
        clip.galley(Pos2::new(sx, ty + gh), gs, tc);
    }
}

fn tip_lines(t: &Tree, i: usize, s: &Settings, info: bool) -> Vec<String> {
    let n = &t.nodes[i];
    if n.is_free {
        return vec![
            format!("Free Space: {}", scan::fmt_percent(t.fs_free, t.fs_total)),
            format!("{} free of {}", scan::fmt_size(t.fs_free), scan::fmt_size(t.fs_total)),
        ];
    }
    let f = s.infotip_flags;
    let mut v = vec![n.name.clone()];
    if !info {
        return v;
    }
    if f & settings::TIP_FULL_PATH != 0 {
        v.push(t.path_of(i).display().to_string());
    }
    if f & settings::TIP_FILENAME != 0 && f & settings::TIP_FULL_PATH == 0 {
        // the name is already the first line
    }
    if f & settings::TIP_SIZE != 0 {
        let mut l = format!("{}  ({} bytes)", scan::fmt_size(n.size), scan::fmt_count(n.size));
        if n.is_dir {
            l.push_str(&format!("   {} files, {} folders", scan::fmt_count(n.files), scan::fmt_count(n.dirs)));
        }
        v.push(l);
    }
    if f & settings::TIP_DATE != 0 {
        let d = scan::fmt_time(n.mtime);
        if !d.is_empty() {
            v.push(d);
        }
    }
    if f & settings::TIP_ATTRS != 0 {
        let a = scan::fmt_attrs(n);
        if !a.is_empty() {
            v.push(a);
        }
    }
    v
}

fn dialog(ctx: &egui::Context, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Window::new(egui::RichText::new(title).font(FontId::new(12.0, FontFamily::Name("bold".into()))))
        .id(egui::Id::unique(("dlg", title)))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(
            Frame::NONE
                .fill(FACE)
                .stroke(Stroke::new(1.0, DKSHADOW))
                .inner_margin(Margin::same(10))
                .shadow(egui::epaint::Shadow { offset: [2, 2], blur: 6, spread: 0, color: Color32::from_black_alpha(90) }),
        )
        .show(ctx, add);
}

fn group(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(4.0);
    let resp = Frame::NONE.inner_margin(Margin { left: 8, right: 8, top: 14, bottom: 8 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
    let r = resp.response.rect;
    let p = ui.painter();
    let er = Rect::from_min_max(Pos2::new(r.left() + 0.5, r.top() + 6.5), Pos2::new(r.right() - 1.5, r.bottom() - 1.5));
    p.rect_stroke(er, 0.0, Stroke::new(1.0, SHADOW), StrokeKind::Middle);
    p.rect_stroke(er.translate(Vec2::new(1.0, 1.0)), 0.0, Stroke::new(1.0, HILITE), StrokeKind::Middle);
    let g = p.layout_no_wrap(title.to_string(), FontId::proportional(12.0), BLACK);
    let tr = Rect::from_min_size(Pos2::new(r.left() + 8.0, r.top()), g.size() + Vec2::new(6.0, 0.0));
    p.rect_filled(tr, 0.0, FACE);
    p.galley(Pos2::new(tr.left() + 3.0, tr.top()), g, BLACK);
}

fn scan_anim(p: &egui::Painter, r: Rect, time: f64) {
    // a magnifier sweeping over a disk drive
    let disk = Rect::from_min_size(Pos2::new(r.left() + 4.0, r.bottom() - 16.0), Vec2::new(34.0, 14.0));
    p.rect_filled(disk, 0.0, Color32::from_rgb(224, 224, 224));
    p.rect_stroke(disk, 0.0, Stroke::new(1.0, DKSHADOW), StrokeKind::Inside);
    p.rect_filled(Rect::from_min_size(Pos2::new(disk.right() - 8.0, disk.top() + 4.0), Vec2::new(4.0, 2.0)), 0.0, Color32::from_rgb(0, 160, 0));
    let t = (time * 2.5).sin() as f32;
    let c = Pos2::new(r.center().x + t * 10.0, r.top() + 12.0);
    p.circle_filled(c, 9.0, Color32::from_rgba_unmultiplied(255, 255, 160, 160));
    p.circle_stroke(c, 9.0, Stroke::new(2.0, BLACK));
    p.line_segment([c + Vec2::new(6.5, 6.5), c + Vec2::new(14.0, 14.0)], Stroke::new(3.0, Color32::from_rgb(160, 0, 0)));
}

/// Small hand-drawn toolbar icons (our own artwork, same ideas as the original's buttons).
fn draw_icon(p: &egui::Painter, cmd: Cmd, r: Rect, enabled: bool) {
    let k = r.width() / 18.0;
    let at = |x: f32, y: f32| Pos2::new(r.left() + x * k, r.top() + y * k);
    let col = |c: Color32| if enabled { c } else { SHADOW };
    let ink = col(BLACK);
    let line = Stroke::new(k.max(1.0), ink);
    let mag = |c: Pos2| {
        p.circle_filled(c, 5.0 * k, col(Color32::WHITE));
        p.circle_stroke(c, 5.0 * k, Stroke::new(1.5 * k, ink));
        p.line_segment([c + Vec2::splat(3.6 * k), c + Vec2::splat(8.5 * k)], Stroke::new(2.5 * k, col(Color32::from_rgb(170, 0, 0))));
    };
    match cmd {
        Cmd::Open => {
            let back = Rect::from_min_max(at(1.0, 5.0), at(15.0, 15.0));
            p.rect_filled(Rect::from_min_max(at(1.0, 3.0), at(7.0, 5.0)), 0.0, col(Color32::from_rgb(255, 255, 160)));
            p.rect_stroke(Rect::from_min_max(at(1.0, 3.0), at(7.0, 5.0)), 0.0, line, StrokeKind::Middle);
            p.rect_filled(back, 0.0, col(Color32::from_rgb(255, 255, 160)));
            p.rect_stroke(back, 0.0, line, StrokeKind::Middle);
            p.add(egui::Shape::convex_polygon(
                vec![at(4.0, 8.0), at(17.0, 8.0), at(15.0, 15.0), at(1.0, 15.0)],
                col(Color32::from_rgb(230, 200, 60)),
                line,
            ));
        }
        Cmd::Reload => {
            let c = at(9.0, 9.0);
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = std::f32::consts::PI * (0.35 + 1.5 * i as f32 / 20.0);
                    c + Vec2::new(a.cos(), -a.sin()) * 6.0 * k
                })
                .collect();
            let end = *pts.last().unwrap();
            let blue = col(Color32::from_rgb(0, 0, 160));
            p.add(egui::Shape::line(pts, Stroke::new(2.2 * k, blue)));
            p.add(egui::Shape::convex_polygon(
                vec![end + Vec2::new(-3.5, -1.0) * k, end + Vec2::new(3.5, -1.0) * k, end + Vec2::new(0.0, 4.0) * k],
                blue,
                Stroke::NONE,
            ));
        }
        Cmd::ZoomFull | Cmd::ZoomIn | Cmd::ZoomOut => {
            let c = at(7.0, 7.0);
            mag(c);
            if cmd != Cmd::ZoomFull {
                p.line_segment([c - Vec2::new(3.0 * k, 0.0), c + Vec2::new(3.0 * k, 0.0)], Stroke::new(1.5 * k, ink));
            }
            if cmd == Cmd::ZoomIn {
                p.line_segment([c - Vec2::new(0.0, 3.0 * k), c + Vec2::new(0.0, 3.0 * k)], Stroke::new(1.5 * k, ink));
            }
        }
        Cmd::FreeSpace => {
            let b = Rect::from_min_max(at(2.0, 2.0), at(16.0, 16.0));
            p.rect_filled(b, 0.0, col(Color32::from_rgb(0, 128, 0)));
            p.rect_filled(Rect::from_min_max(at(4.0, 4.0), at(14.0, 11.0)), 0.0, col(Color32::WHITE));
            p.rect_filled(Rect::from_min_max(at(2.0, 12.0), at(16.0, 16.0)), 0.0, col(Color32::from_rgb(200, 0, 0)));
            p.rect_stroke(b, 0.0, line, StrokeKind::Middle);
        }
        Cmd::Run => {
            p.add(egui::Shape::convex_polygon(
                vec![at(4.0, 2.0), at(16.0, 9.0), at(4.0, 16.0)],
                col(Color32::from_rgb(255, 230, 0)),
                line,
            ));
        }
        Cmd::Delete => {
            let s = Stroke::new(3.0 * k, col(Color32::from_rgb(200, 0, 0)));
            p.line_segment([at(3.0, 3.0), at(15.0, 15.0)], s);
            p.line_segment([at(15.0, 3.0), at(3.0, 15.0)], s);
        }
        Cmd::Setup => {
            let b = Rect::from_min_max(at(2.0, 4.0), at(14.0, 16.0));
            p.rect_filled(b, 0.0, col(Color32::WHITE));
            p.rect_stroke(b, 0.0, line, StrokeKind::Middle);
            p.add(egui::Shape::line(vec![at(4.0, 9.0), at(8.0, 14.0), at(17.0, 1.0)], Stroke::new(2.0 * k, col(Color32::from_rgb(180, 0, 0)))));
        }
        Cmd::About => {
            let a = Rect::from_min_max(at(1.0, 1.0), at(11.0, 11.0));
            let b = Rect::from_min_max(at(7.0, 7.0), at(17.0, 17.0));
            p.rect_filled(a, 0.0, col(Color32::from_rgb(230, 0, 0)));
            p.rect_stroke(a, 0.0, line, StrokeKind::Middle);
            p.rect_filled(b, 0.0, col(Color32::from_rgb(0, 200, 0)));
            p.rect_stroke(b, 0.0, line, StrokeKind::Middle);
        }
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let regular = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
    ];
    let bold = [
        "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/dejavu/DejaVuSans-Bold.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
    ];
    let load = |list: &[&str]| list.iter().find_map(|p| std::fs::read(p).ok());
    if let Some(b) = load(&regular) {
        fonts.font_data.insert("sys".into(), Arc::new(egui::FontData::from_owned(b)));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "sys".into());
    }
    let mut bold_family = Vec::new();
    if let Some(b) = load(&bold) {
        fonts.font_data.insert("sys-bold".into(), Arc::new(egui::FontData::from_owned(b)));
        bold_family.push("sys-bold".to_string());
    }
    bold_family.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name("bold".into()), bold_family);
    ctx.set_fonts(fonts);
}

fn save_png(path: &Path, img: &egui::ColorImage) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(dir);
        }
    }
    let f = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(|e| e.to_string())?;
    let mut data = Vec::with_capacity(img.pixels.len() * 4);
    for p in &img.pixels {
        let [r, g, b, _] = p.to_array();
        data.extend_from_slice(&[r, g, b, 255]);
    }
    wr.write_image_data(&data).map_err(|e| e.to_string())?;
    Ok(())
}

fn find_path(t: &Tree, path: &Path) -> Option<usize> {
    let rel = path.strip_prefix(&t.root_path).ok()?;
    let mut cur = t.root();
    for comp in rel.components() {
        let name = comp.as_os_str().to_string_lossy();
        cur = *t.nodes[cur].children.iter().find(|&&c| t.nodes[c].name == name)?;
    }
    Some(cur)
}

fn expand_home(s: &str) -> String {
    if let Some(rest) = s.strip_prefix('~') {
        if let Ok(h) = std::env::var("HOME") {
            return format!("{}{}", h, rest);
        }
    }
    s.to_string()
}

fn list_mounts() -> Vec<Mount> {
    let mut v = Vec::new();
    if let Ok(h) = std::env::var("HOME") {
        let p = PathBuf::from(&h);
        let (total, free) = scan::fs_space(&p);
        v.push(Mount { label: format!("Home folder ({})", h), path: p, fstype: "home".into(), total, free });
    }
    const REAL: [&str; 16] = [
        "ext2", "ext3", "ext4", "btrfs", "xfs", "vfat", "exfat", "ntfs", "ntfs3", "fuseblk", "f2fs", "zfs", "nfs", "nfs4",
        "cifs", "reiserfs",
    ];
    let mut seen = std::collections::HashSet::new();
    if let Ok(text) = std::fs::read_to_string("/proc/mounts") {
        for line in text.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 3 || !REAL.contains(&f[2]) {
                continue;
            }
            let mp = f[1].replace("\\040", " ");
            if mp.starts_with("/snap") || mp.starts_with("/var/snap") || (mp.starts_with("/boot/") && mp != "/boot/efi") {
                continue;
            }
            if !seen.insert(mp.clone()) {
                continue;
            }
            let p = PathBuf::from(&mp);
            let (total, free) = scan::fs_space(&p);
            v.push(Mount { label: mp, path: p, fstype: f[2].to_string(), total, free });
        }
    }
    v
}
