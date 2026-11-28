//! Walks a folder tree and records sizes. Runs on a background thread.
//! Unix and Windows (remote agent) share this module behind `cfg`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    /// The "<Free Space>" pseudo item, only under the scan root.
    pub is_free: bool,
    pub parent: Option<usize>,
    /// Children, largest first.
    pub children: Vec<usize>,
    /// Scan root = -1, its children 0, grandchildren 1 ...
    pub depth: i32,
    pub mtime: Option<SystemTime>,
    pub mode: u32,
    pub files: u64,
    pub dirs: u64,
}

pub struct Tree {
    pub nodes: Vec<Node>,
    pub root_path: PathBuf,
    pub fs_total: u64,
    pub fs_free: u64,
    pub free_node: Option<usize>,
}

impl Tree {
    pub fn path_of(&self, idx: usize) -> PathBuf {
        let mut parts = Vec::new();
        let mut cur = Some(idx);
        while let Some(i) = cur {
            let n = &self.nodes[i];
            if n.parent.is_none() {
                break;
            }
            parts.push(n.name.clone());
            cur = n.parent;
        }
        let mut p = self.root_path.clone();
        for part in parts.iter().rev() {
            p.push(part);
        }
        p
    }

    pub fn root(&self) -> usize {
        0
    }

    /// Remove a node (after deleting it from disk) and fix the sizes above it.
    pub fn remove(&mut self, idx: usize) {
        let size = self.nodes[idx].size;
        let files = if self.nodes[idx].is_dir {
            self.nodes[idx].files
        } else {
            1
        };
        let dirs = if self.nodes[idx].is_dir {
            self.nodes[idx].dirs + 1
        } else {
            0
        };
        if let Some(p) = self.nodes[idx].parent {
            self.nodes[p].children.retain(|&c| c != idx);
            let mut cur = Some(p);
            while let Some(i) = cur {
                let n = &mut self.nodes[i];
                n.size = n.size.saturating_sub(size);
                n.files = n.files.saturating_sub(files);
                n.dirs = n.dirs.saturating_sub(dirs);
                cur = n.parent;
            }
            let mut kids = std::mem::take(&mut self.nodes[p].children);
            kids.sort_by(|a, b| self.nodes[*b].size.cmp(&self.nodes[*a].size));
            self.nodes[p].children = kids;
        }
        self.nodes[idx].size = 0;
        self.nodes[idx].children.clear();
    }

    pub fn is_ancestor(&self, anc: usize, mut idx: usize) -> bool {
        loop {
            if idx == anc {
                return true;
            }
            match self.nodes[idx].parent {
                Some(p) => idx = p,
                None => return false,
            }
        }
    }
}

#[derive(Default)]
pub struct Progress {
    pub folders: AtomicU64,
    pub files: AtomicU64,
    pub cancel: AtomicBool,
    pub current: Mutex<String>,
}

/// Volume / device id so the walk stays on one filesystem (one drive).
#[cfg(unix)]
fn volume_id(_path: &Path, meta: &fs::Metadata) -> u64 {
    meta.dev()
}

/// Drive / UNC prefix string (stable; avoids unstable `volume_serial_number`).
#[cfg(windows)]
fn volume_id(path: &Path, _meta: &fs::Metadata) -> String {
    use std::path::Component;
    match path.components().next() {
        Some(Component::Prefix(p)) => format!("{:?}", p.as_os_str()),
        _ => String::new(),
    }
}

#[cfg(unix)]
fn file_mode(meta: &fs::Metadata) -> u32 {
    meta.mode()
}

#[cfg(windows)]
fn file_mode(meta: &fs::Metadata) -> u32 {
    meta.file_attributes()
}

/// (total, free) bytes of the file system holding `path`.
#[cfg(unix)]
pub fn fs_space(path: &Path) -> (u64, u64) {
    use std::ffi::CString;
    let Ok(c) = CString::new(path.as_os_str().as_encoded_bytes()) else {
        return (0, 0);
    };
    unsafe {
        let mut st: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut st) == 0 {
            let fr = st.f_frsize as u64;
            (st.f_blocks as u64 * fr, st.f_bavail as u64 * fr)
        } else {
            (0, 0)
        }
    }
}

#[cfg(windows)]
pub fn fs_space(path: &Path) -> (u64, u64) {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            lp_directory_name: *const u16,
            lp_free_bytes_available_to_caller: *mut u64,
            lp_total_number_of_bytes: *mut u64,
            lp_total_number_of_free_bytes: *mut u64,
        ) -> i32;
    }
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut free_avail: u64 = 0;
    let mut total: u64 = 0;
    let mut free_total: u64 = 0;
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_avail,
            &mut total,
            &mut free_total,
        )
    };
    if ok != 0 {
        (total, free_avail)
    } else {
        (0, 0)
    }
}

pub fn scan(root: PathBuf, prog: Arc<Progress>) -> Option<Tree> {
    let root = fs::canonicalize(&root).unwrap_or(root);
    let meta = fs::metadata(&root).ok()?;
    if !meta.is_dir() {
        return None;
    }
    let dev = volume_id(&root, &meta);
    let mut nodes = vec![Node {
        name: root.to_string_lossy().to_string(),
        size: 0,
        is_dir: true,
        is_free: false,
        parent: None,
        children: Vec::new(),
        depth: -1,
        mtime: meta.modified().ok(),
        mode: file_mode(&meta),
        files: 0,
        dirs: 0,
    }];
    walk(&root, 0, &dev, &mut nodes, &prog);
    if prog.cancel.load(Ordering::Relaxed) {
        return None;
    }
    let (fs_total, fs_free) = fs_space(&root);
    let free_idx = nodes.len();
    // Not counted in the folder's own size; added at layout time when "Free Space" is on.
    nodes.push(Node {
        name: String::new(),
        size: fs_free,
        is_dir: false,
        is_free: true,
        parent: Some(0),
        children: Vec::new(),
        depth: 0,
        mtime: None,
        mode: 0,
        files: 0,
        dirs: 0,
    });
    Some(Tree {
        nodes,
        root_path: root,
        fs_total,
        fs_free,
        free_node: Some(free_idx),
    })
}

#[cfg(unix)]
type VolId = u64;
#[cfg(windows)]
type VolId = String;

fn walk(path: &Path, idx: usize, dev: &VolId, nodes: &mut Vec<Node>, prog: &Progress) {
    if prog.cancel.load(Ordering::Relaxed) {
        return;
    }
    prog.folders.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut cur) = prog.current.try_lock() {
        *cur = path.to_string_lossy().to_string();
    }
    let depth = nodes[idx].depth + 1;
    let Ok(rd) = fs::read_dir(path) else {
        return;
    };
    let mut kids = Vec::new();
    for entry in rd.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let ft = meta.file_type();
        let is_dir = ft.is_dir();
        let size = if ft.is_file() { meta.len() } else { 0 };
        let ci = nodes.len();
        nodes.push(Node {
            name: entry.file_name().to_string_lossy().to_string(),
            size,
            is_dir,
            is_free: false,
            parent: Some(idx),
            children: Vec::new(),
            depth,
            mtime: meta.modified().ok(),
            mode: file_mode(&meta),
            files: 0,
            dirs: 0,
        });
        kids.push(ci);
        if is_dir {
            // stay on one file system, like one drive in the original
            let child = entry.path();
            if &volume_id(&child, &meta) == dev {
                walk(&child, ci, dev, nodes, prog);
            }
        } else {
            prog.files.fetch_add(1, Ordering::Relaxed);
        }
    }
    let (mut total, mut files, mut dirs) = (0u64, 0u64, 0u64);
    for &k in &kids {
        let n = &nodes[k];
        total += n.size;
        if n.is_dir {
            dirs += 1 + n.dirs;
            files += n.files;
        } else {
            files += 1;
        }
    }
    kids.sort_by(|a, b| {
        nodes[*b]
            .size
            .cmp(&nodes[*a].size)
            .then_with(|| nodes[*a].name.cmp(&nodes[*b].name))
    });
    let n = &mut nodes[idx];
    n.size = total;
    n.files = files;
    n.dirs = dirs;
    n.children = kids;
}

/// "%lu.%01lu %s" with bytes / Kb / Mb / Gb (Tb added).
pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["Kb", "Mb", "Gb", "Tb"];
    if bytes < 1024 {
        return format!("{} bytes", bytes);
    }
    let mut unit = 0;
    let mut div: u64 = 1024;
    while unit + 1 < UNITS.len() && bytes >= div * 1024 {
        div *= 1024;
        unit += 1;
    }
    let tenths = (bytes as u128 * 10 / div as u128) as u64;
    format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
}

/// "%lu.%01lu%%"
pub fn fmt_percent(part: u64, whole: u64) -> String {
    if whole == 0 {
        return "0.0%".into();
    }
    let t = (part as u128 * 1000 / whole as u128) as u64;
    format!("{}.{}%", t / 10, t % 10)
}

pub fn fmt_count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "%02d %s %04d   %d:%02d:%02d" as in the original.
#[cfg(unix)]
pub fn fmt_time(t: Option<SystemTime>) -> String {
    let Some(t) = t else {
        return String::new();
    };
    let Ok(d) = t.duration_since(SystemTime::UNIX_EPOCH) else {
        return String::new();
    };
    let secs = d.as_secs() as libc::time_t;
    unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return String::new();
        }
        const MON: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!(
            "{:02} {} {:04}   {}:{:02}:{:02}",
            tm.tm_mday,
            MON[tm.tm_mon.clamp(0, 11) as usize],
            tm.tm_year + 1900,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

#[cfg(windows)]
pub fn fmt_time(t: Option<SystemTime>) -> String {
    let Some(t) = t else {
        return String::new();
    };
    let Ok(d) = t.duration_since(SystemTime::UNIX_EPOCH) else {
        return String::new();
    };
    // Agent / Windows build does not drive the GUI clock string; keep a stable UTC form.
    let secs = d.as_secs();
    let days = secs / 86400;
    let tod = secs % 86400;
    let h = tod / 3600;
    let m = (tod % 3600) / 60;
    let s = tod % 60;
    let (y, mon, day) = civil_from_days(days as i64);
    const MON: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{:02} {} {:04}   {}:{:02}:{:02}",
        day,
        MON[(mon - 1).clamp(0, 11) as usize],
        y,
        h,
        m,
        s
    )
}

#[cfg(windows)]
fn civil_from_days(days: i64) -> (i32, i32, i32) {
    // Howard Hinnant algorithm (UTC).
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64 + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as i32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as i32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

pub fn fmt_attrs(n: &Node) -> String {
    let mut a = Vec::new();
    if n.is_dir {
        a.push("Folder");
    }
    if n.name.starts_with('.') {
        a.push("Hidden");
    }
    #[cfg(unix)]
    {
        if n.mode & 0o222 == 0 {
            a.push("Read-Only");
        }
        if !n.is_dir && n.mode & 0o111 != 0 {
            a.push("Executable");
        }
    }
    #[cfg(windows)]
    {
        // FILE_ATTRIBUTE_READONLY = 0x1, HIDDEN = 0x2
        if n.mode & 0x1 != 0 {
            a.push("Read-Only");
        }
        if n.mode & 0x2 != 0 {
            a.push("Hidden");
        }
    }
    a.join(" ")
}
