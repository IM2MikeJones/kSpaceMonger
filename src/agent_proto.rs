//! NDJSON protocol between parent GUI and remote `ksm-agent`.

use crate::scan::{Node, Progress, Tree};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::{Duration, UNIX_EPOCH};

pub const PROTO_VERSION: u32 = 1;
/// Redeploy when this differs from the remote binary's `--version` (crate version).
pub const AGENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello,
    Scan { path: String },
    Cancel,
    Quit,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    HelloOk {
        proto: u32,
        version: String,
        arch: String,
    },
    Progress {
        folders: u64,
        files: u64,
        current: String,
    },
    Done {
        root_path: String,
        fs_total: u64,
        fs_free: u64,
        free_node: Option<usize>,
        nodes: Vec<WireNode>,
    },
    Error {
        code: String,
        msg: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireNode {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
    pub is_free: bool,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub depth: i32,
    pub mode: u32,
    pub files: u64,
    pub dirs: u64,
    pub mtime_secs: Option<u64>,
}

impl WireNode {
    pub fn from_node(n: &Node) -> Self {
        Self {
            name: n.name.clone(),
            size: n.size,
            is_dir: n.is_dir,
            is_free: n.is_free,
            parent: n.parent,
            children: n.children.clone(),
            depth: n.depth,
            mode: n.mode,
            files: n.files,
            dirs: n.dirs,
            mtime_secs: n.mtime.and_then(|t| t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())),
        }
    }

    pub fn into_node(self) -> Node {
        Node {
            name: self.name,
            size: self.size,
            is_dir: self.is_dir,
            is_free: self.is_free,
            parent: self.parent,
            children: self.children,
            depth: self.depth,
            mtime: self
                .mtime_secs
                .map(|s| UNIX_EPOCH + Duration::from_secs(s)),
            mode: self.mode,
            files: self.files,
            dirs: self.dirs,
        }
    }
}

pub fn tree_to_done(tree: &Tree) -> ServerMsg {
    ServerMsg::Done {
        root_path: tree.root_path.display().to_string(),
        fs_total: tree.fs_total,
        fs_free: tree.fs_free,
        free_node: tree.free_node,
        nodes: tree.nodes.iter().map(WireNode::from_node).collect(),
    }
}

pub fn tree_from_done(msg: ServerMsg) -> Option<Tree> {
    let ServerMsg::Done {
        root_path,
        fs_total,
        fs_free,
        free_node,
        nodes,
    } = msg
    else {
        return None;
    };
    Some(Tree {
        nodes: nodes.into_iter().map(WireNode::into_node).collect(),
        root_path: PathBuf::from(root_path),
        fs_total,
        fs_free,
        free_node,
    })
}

pub fn progress_msg(prog: &Progress) -> ServerMsg {
    let current = prog
        .current
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    ServerMsg::Progress {
        folders: prog.folders.load(Ordering::Relaxed),
        files: prog.files.load(Ordering::Relaxed),
        current,
    }
}

pub fn write_line(out: &mut impl std::io::Write, msg: &ServerMsg) -> std::io::Result<()> {
    serde_json::to_writer(&mut *out, msg)?;
    out.write_all(b"\n")?;
    out.flush()
}

pub fn read_client_line(line: &str) -> Result<ClientMsg, String> {
    serde_json::from_str(line.trim()).map_err(|e| e.to_string())
}

pub fn read_server_line(line: &str) -> Result<ServerMsg, String> {
    serde_json::from_str(line.trim()).map_err(|e| e.to_string())
}

pub fn host_arch() -> String {
    std::env::consts::ARCH.to_string()
}
