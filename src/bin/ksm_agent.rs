//! Headless remote scanner for kSpaceMonger (NDJSON over stdin/stdout).

use kspacemonger::agent_proto::{
    self, read_client_line, write_line, ClientMsg, ServerMsg, AGENT_VERSION, PROTO_VERSION,
};
use kspacemonger::scan::{self, Progress};
use std::io::{self, BufRead};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("--version") | Some("-V") => {
            println!("{AGENT_VERSION}");
            return;
        }
        Some("--agent") => run_agent(),
        Some("-h") | Some("--help") | None => {
            eprintln!("Usage: ksm-agent --agent | --version");
            std::process::exit(2);
        }
        Some(other) => {
            eprintln!("unknown argument: {other}");
            std::process::exit(2);
        }
    }
}

fn run_agent() {
    // Shared stdout lock — must NOT be held across scan::scan or the progress
    // ticker cannot flush NDJSON and the parent UI stays at 0 forever.
    let out = Arc::new(Mutex::new(io::stdout()));
    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();

    while let Some(Ok(line)) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        let msg = match read_client_line(&line) {
            Ok(m) => m,
            Err(e) => {
                let mut g = out.lock().unwrap();
                let _ = write_line(
                    &mut *g,
                    &ServerMsg::Error {
                        code: "bad_json".into(),
                        msg: e,
                    },
                );
                continue;
            }
        };
        match msg {
            ClientMsg::Hello => {
                let mut g = out.lock().unwrap();
                let _ = write_line(
                    &mut *g,
                    &ServerMsg::HelloOk {
                        proto: PROTO_VERSION,
                        version: AGENT_VERSION.to_string(),
                        arch: agent_proto::host_arch(),
                    },
                );
            }
            ClientMsg::Scan { path } => {
                let progress = Arc::new(Progress::default());
                let p2 = progress.clone();
                let out_tick = out.clone();
                let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
                let stop2 = stop.clone();
                let tick = thread::spawn(move || {
                    while !stop2.load(Ordering::Relaxed) {
                        if let Ok(mut g) = out_tick.lock() {
                            let _ = write_line(&mut *g, &agent_proto::progress_msg(&p2));
                        }
                        thread::sleep(Duration::from_millis(200));
                    }
                });
                let path_display = path.clone();
                let tree = scan::scan(PathBuf::from(path), progress.clone());
                stop.store(true, Ordering::Relaxed);
                let _ = tick.join();
                let mut g = out.lock().unwrap();
                if progress.cancel.load(Ordering::Relaxed) {
                    let _ = write_line(
                        &mut *g,
                        &ServerMsg::Error {
                            code: "cancelled".into(),
                            msg: "cancelled".into(),
                        },
                    );
                } else if let Some(tree) = tree {
                    let _ = write_line(&mut *g, &agent_proto::progress_msg(&progress));
                    let _ = write_line(&mut *g, &agent_proto::tree_to_done(&tree));
                } else {
                    let _ = write_line(
                        &mut *g,
                        &ServerMsg::Error {
                            code: "scan_failed".into(),
                            msg: format!("scan failed or path not a folder: {path_display}"),
                        },
                    );
                }
            }
            ClientMsg::Cancel => {}
            ClientMsg::Quit => break,
        }
    }
}
