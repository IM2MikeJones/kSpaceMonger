//! SSH remote agent deploy and scan (Linux + Windows x86_64).
//! Never walks fuse.sshfs locally. OS is detected over SSH, never from mount names.

use crate::agent_proto::{self, read_server_line, ClientMsg, ServerMsg, AGENT_VERSION};
use crate::scan::{Progress, Tree};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

/// Parsed sshfs mount from /proc/mounts.
#[derive(Clone, Debug)]
pub struct SshfsTarget {
    /// Local mountpoint (e.g. `/mnt/remote`).
    pub local_mount: PathBuf,
    /// SSH host alias or name from the sshfs device field (not used for OS detection).
    pub host: String,
    /// Path on the remote host (e.g. `/` or `D:/`).
    pub remote_root: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteOs {
    Linux,
    Windows,
}

#[derive(Clone, Debug)]
pub struct ProbeInfo {
    pub os: RemoteOs,
    /// Human tip, e.g. "Linux x86_64" / "Windows AMD64".
    pub label: String,
    /// Prefer user cache dir over temp.
    pub home_ok: bool,
    /// Expanded home / profile for scp (no $vars).
    pub home_path: String,
    /// Temp dir if home not writable (Linux `/tmp/...` parent or Windows TEMP).
    pub temp_path: String,
}

#[derive(Clone, Debug, Default)]
pub struct StatusChips {
    pub visible: bool,
    pub ssh: String,
    pub agent: String,
    pub scan: String,
    pub store: String,
    pub tip: String,
}

impl StatusChips {
    pub fn local() -> Self {
        Self::default()
    }

    pub fn set(&mut self, ssh: &str, agent: &str, scan: &str, store: &str, tip: impl Into<String>) {
        self.visible = true;
        self.ssh = ssh.into();
        self.agent = agent.into();
        self.scan = scan.into();
        self.store = store.into();
        self.tip = tip.into();
    }
}

#[derive(Debug)]
pub enum RemoteError {
    Unsupported { detail: String },
    Ssh { detail: String },
    Deploy { detail: String },
    Agent { detail: String },
    Scan { detail: String },
}

impl RemoteError {
    pub fn user_message(&self) -> String {
        match self {
            Self::Unsupported { detail } => format!("Unsupported remote: {detail}"),
            Self::Ssh { detail } => format!("SSH: {detail}"),
            Self::Deploy { detail } => format!("Agent deploy: {detail}"),
            Self::Agent { detail } => format!("Agent: {detail}"),
            Self::Scan { detail } => format!("Scan: {detail}"),
        }
    }
}

fn probe_cache() -> &'static Mutex<HashMap<String, ProbeInfo>> {
    static CACHE: OnceLock<Mutex<HashMap<String, ProbeInfo>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// sshfs device is `host:path`. For a Windows drive, the path is often `/D:/…`
/// (slash before the drive letter). That form is not a folder on Windows — strip it.
fn normalize_sshfs_remote_path(path: &str) -> String {
    if path.is_empty() {
        return "/".into();
    }
    let b = path.as_bytes();
    // `/D:` or `/D:/…` → `D:` / `D:/…`
    if b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        return path[1..].to_string();
    }
    path.to_string()
}

/// Parse fuse.sshfs (and fuse.sshfs-like) entries. Device field is `host:path`.
pub fn parse_sshfs_line(device: &str, mountpoint: &str, fstype: &str) -> Option<SshfsTarget> {
    let ft = fstype.to_ascii_lowercase();
    if ft != "fuse.sshfs" && ft != "fuse.sshfs#sshfs" && !ft.contains("sshfs") {
        return None;
    }
    let device = device.replace("\\040", " ");
    let (host, remote_root) = match device.split_once(':') {
        Some((h, p)) if !h.is_empty() => {
            let raw = if p.is_empty() { "/" } else { p };
            (h.to_string(), normalize_sshfs_remote_path(raw))
        }
        _ => return None,
    };
    Some(SshfsTarget {
        local_mount: PathBuf::from(mountpoint.replace("\\040", " ")),
        host,
        remote_root,
    })
}

/// Join remote root + relative path using `/` (sshfs / OpenSSH accept forward slashes on Windows).
fn join_remote(base: &str, rel: &Path) -> String {
    if rel.as_os_str().is_empty() {
        return base.to_string();
    }
    let rel_s = rel.to_string_lossy().replace('\\', "/");
    let base = base.trim_end_matches(['/', '\\']);
    if base.is_empty() {
        rel_s
    } else {
        format!("{base}/{rel_s}")
    }
}

/// If `path` is under an sshfs mount, return that target and the path on the remote.
pub fn resolve_remote(path: &Path, mounts: &[(PathBuf, SshfsTarget)]) -> Option<(SshfsTarget, String)> {
    let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut best: Option<&SshfsTarget> = None;
    for (mp, t) in mounts {
        if canon.starts_with(mp) || path.starts_with(mp) {
            if best.as_ref().map(|b| b.local_mount.as_os_str().len()).unwrap_or(0) < mp.as_os_str().len()
            {
                best = Some(t);
            }
        }
    }
    let t = best?.clone();
    let rel = path.strip_prefix(&t.local_mount).unwrap_or(Path::new(""));
    let remote = join_remote(&t.remote_root, rel);
    Some((t, remote))
}

fn agent_candidates(windows: bool) -> Vec<PathBuf> {
    let name = if windows { "ksm-agent.exe" } else { "ksm-agent" };
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Packaged / same-folder layout (AppImage, .deb, copy next to GUI).
            candidates.push(dir.join(name));
            candidates.push(dir.join("lib").join("kspacemonger").join(name));
            if let Some(grand) = dir.parent() {
                candidates.push(grand.join("lib").join("kspacemonger").join(name));
                candidates.push(grand.join("libexec").join("kspacemonger").join(name));
                // Cargo: …/target/{debug,release}/kSpaceMonger → …/target/<triple>/{release,debug}/
                if windows {
                    for triple in ["x86_64-pc-windows-msvc", "x86_64-pc-windows-gnu"] {
                        for profile in ["release", "debug"] {
                            candidates.push(grand.join(triple).join(profile).join(name));
                        }
                    }
                }
            }
        }
    }
    candidates.push(PathBuf::from(format!("/usr/lib/kspacemonger/{name}")));
    candidates.push(PathBuf::from(format!("/usr/libexec/kspacemonger/{name}")));
    // CWD-relative (dev: run from crate root).
    if let Ok(cwd) = std::env::current_dir() {
        if windows {
            for triple in ["x86_64-pc-windows-msvc", "x86_64-pc-windows-gnu"] {
                for profile in ["release", "debug"] {
                    candidates.push(cwd.join("target").join(triple).join(profile).join(name));
                }
            }
            candidates.push(cwd.join("target/release").join(name));
            candidates.push(cwd.join("target/debug").join(name));
        } else {
            candidates.push(cwd.join("target/release").join(name));
            candidates.push(cwd.join("target/debug").join(name));
        }
    }
    candidates
}

pub fn local_agent_binary(os: RemoteOs) -> Option<PathBuf> {
    agent_candidates(os == RemoteOs::Windows)
        .into_iter()
        .find(|p| p.is_file())
}

fn ssh_base(host: &str) -> Command {
    let mut c = Command::new("ssh");
    c.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=8",
        "-o",
        "StrictHostKeyChecking=accept-new",
        host,
    ]);
    c
}

fn ssh_fail(host: &str, out: &std::process::Output) -> RemoteError {
    let err = String::from_utf8_lossy(&out.stderr);
    let msg = err.trim();
    if msg.contains("Permission denied") || msg.contains("publickey") {
        return RemoteError::Ssh {
            detail: format!("Auth failed for `{host}`"),
        };
    }
    RemoteError::Ssh {
        detail: if msg.is_empty() {
            format!("Unreachable `{host}`")
        } else {
            format!("Unreachable `{host}`: {msg}")
        },
    }
}

fn looks_windows_uname(s: &str) -> bool {
    let u = s.to_ascii_uppercase();
    u.contains("MINGW")
        || u.contains("CYGWIN")
        || u.contains("MSYS")
        || u.contains("WINDOWS_NT")
        || u.starts_with("WINDOWS")
}

fn arch_ok_x86_64(arch: &str) -> bool {
    let a = arch.to_ascii_lowercase();
    a.contains("x86_64") || a.contains("amd64") || a == "x64"
}

/// SSH OS/arch probe. Cached per host for the process lifetime. Never uses mount names.
pub fn ssh_probe(host: &str) -> Result<ProbeInfo, RemoteError> {
    if let Ok(cache) = probe_cache().lock() {
        if let Some(p) = cache.get(host) {
            return Ok(p.clone());
        }
    }

    let info = ssh_probe_uncached(host)?;
    if let Ok(mut cache) = probe_cache().lock() {
        cache.insert(host.to_string(), info.clone());
    }
    Ok(info)
}

fn ssh_probe_uncached(host: &str) -> Result<ProbeInfo, RemoteError> {
    // Connectivity check first (empty remote command / true-equivalent).
    // Windows OpenSSH may use cmd or PowerShell — avoid shell-specific operators here.
    let ping = ssh_base(host)
        .arg("echo KSM_PROBE_OK")
        .output()
        .map_err(|e| RemoteError::Ssh {
            detail: format!("cannot run ssh: {e}"),
        })?;
    if !ping.status.success()
        || !String::from_utf8_lossy(&ping.stdout).contains("KSM_PROBE_OK")
    {
        return Err(ssh_fail(host, &ping));
    }

    // 1) uname — Linux or some Windows OpenSSH / Git environments.
    // Do not treat failure as unreachable: PowerShell default shells reject `||` / `uname`.
    let uname_out = ssh_base(host).arg("uname -sm").output().ok();
    let uname = uname_out
        .as_ref()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(str::trim)
                .find(|l| {
                    let u = l.to_ascii_lowercase();
                    u.contains("linux")
                        || u.contains("mingw")
                        || u.contains("cygwin")
                        || u.contains("msys")
                        || u.contains("windows")
                })
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default();
    let uname_l = uname.to_ascii_lowercase();

    if uname_l.contains("linux") {
        if !arch_ok_x86_64(&uname_l) {
            return Err(RemoteError::Unsupported {
                detail: format!("need x86_64 agent ({uname})"),
            });
        }
        return linux_paths(host, &uname);
    }

    if looks_windows_uname(&uname) {
        return windows_probe(host, Some(&uname));
    }

    // 2) Native Windows OpenSSH (cmd.exe env) — version-agnostic
    windows_probe(host, None)
}

fn linux_paths(host: &str, uname: &str) -> Result<ProbeInfo, RemoteError> {
    let out = ssh_base(host)
        .arg(
            r#"printf 'HOME=%s\n' "$HOME"; test -w "$HOME" && echo HOME_OK || echo HOME_NO; test -w /tmp && echo TMP_OK || echo TMP_NO"#,
        )
        .output()
        .map_err(|e| RemoteError::Ssh {
            detail: format!("home probe failed: {e}"),
        })?;
    if !out.status.success() {
        return Err(ssh_fail(host, &out));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut home_path = String::new();
    let mut home_ok = false;
    let mut tmp_ok = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("HOME=") {
            home_path = rest.to_string();
        } else if line.contains("HOME_OK") {
            home_ok = true;
        } else if line.contains("HOME_NO") {
            home_ok = false;
        } else if line.contains("TMP_OK") {
            tmp_ok = true;
        } else if line.contains("TMP_NO") {
            tmp_ok = false;
        }
    }
    if !home_ok && !tmp_ok {
        return Err(RemoteError::Deploy {
            detail: "Not writable — cannot install agent (~/.cache or /tmp)".into(),
        });
    }
    if home_ok && home_path.is_empty() {
        return Err(RemoteError::Deploy {
            detail: "could not resolve remote HOME".into(),
        });
    }
    Ok(ProbeInfo {
        os: RemoteOs::Linux,
        label: uname.to_string(),
        home_ok,
        home_path,
        temp_path: "/tmp/kspacemonger-agent".into(),
    })
}

fn windows_probe(host: &str, uname_hint: Option<&str>) -> Result<ProbeInfo, RemoteError> {
    let out = ssh_base(host)
        .arg(
            r#"cmd.exe /c "echo OS=%OS%& echo ARCH=%PROCESSOR_ARCHITECTURE%& echo PROFILE=%USERPROFILE%& echo TEMPDIR=%TEMP%""#,
        )
        .output()
        .map_err(|e| RemoteError::Ssh {
            detail: format!("cannot run ssh: {e}"),
        })?;
    if !out.status.success() && out.stdout.is_empty() {
        return Err(ssh_fail(host, &out));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut os_val = String::new();
    let mut arch = String::new();
    let mut profile = String::new();
    let mut temp = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("OS=") {
            os_val = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("ARCH=") {
            arch = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("PROFILE=") {
            profile = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("TEMPDIR=") {
            temp = v.trim().to_string();
        }
    }

    let is_win = os_val.eq_ignore_ascii_case("Windows_NT")
        || uname_hint.map(looks_windows_uname).unwrap_or(false)
        || (!profile.is_empty() && profile.contains(':') && !arch.is_empty());
    if !is_win {
        let detail = if uname_hint.unwrap_or("").is_empty() && os_val.is_empty() {
            "could not detect remote OS (not Linux x86_64 or Windows)".into()
        } else {
            format!(
                "unsupported remote OS (uname={}; OS={})",
                uname_hint.unwrap_or("-"),
                if os_val.is_empty() { "-" } else { &os_val }
            )
        };
        return Err(RemoteError::Unsupported { detail });
    }
    if !arch_ok_x86_64(&arch) {
        return Err(RemoteError::Unsupported {
            detail: format!("need x86_64 Windows agent (PROCESSOR_ARCHITECTURE={arch})"),
        });
    }
    if profile.is_empty() && temp.is_empty() {
        return Err(RemoteError::Deploy {
            detail: "could not resolve USERPROFILE or TEMP".into(),
        });
    }

    // Writability: try creating the cache dir under profile; fall back to TEMP.
    let home_ok = if !profile.is_empty() {
        let cache = win_cmd_path(&format!("{}\\.cache\\kspacemonger", profile));
        let test = ssh_base(host)
            .arg(format!(
                r#"cmd.exe /c "mkdir {cache} 2>NUL & if exist {cache}\ (echo OK) else (echo FAIL)""#
            ))
            .output()
            .ok();
        test.map(|o| String::from_utf8_lossy(&o.stdout).contains("OK"))
            .unwrap_or(false)
    } else {
        false
    };
    if !home_ok && temp.is_empty() {
        return Err(RemoteError::Deploy {
            detail: "Not writable — cannot install agent (USERPROFILE or TEMP)".into(),
        });
    }

    let label = format!(
        "Windows {}{}",
        if arch.is_empty() { "AMD64" } else { &arch },
        uname_hint
            .map(|u| format!(" ({u})"))
            .unwrap_or_default()
    );
    Ok(ProbeInfo {
        os: RemoteOs::Windows,
        label,
        home_ok,
        home_path: profile,
        temp_path: if temp.is_empty() {
            String::new()
        } else {
            format!("{temp}\\kspacemonger")
        },
    })
}

fn remote_agent_paths(probe: &ProbeInfo) -> (String /*dir*/, String /*agent*/, String /*scp agent.new*/) {
    match probe.os {
        RemoteOs::Linux => {
            if probe.home_ok {
                let dir = format!("{}/.cache/kspacemonger", probe.home_path);
                let agent = format!("{dir}/ksm-agent");
                let scp = format!("{agent}.new");
                (dir, agent, scp)
            } else {
                let dir = probe.temp_path.clone();
                let agent = format!("{dir}/ksm-agent");
                let scp = format!("{agent}.new");
                (dir, agent, scp)
            }
        }
        RemoteOs::Windows => {
            if probe.home_ok {
                let dir = format!("{}\\.cache\\kspacemonger", probe.home_path);
                let agent = format!("{dir}\\ksm-agent.exe");
                let scp = format!("{agent}.new");
                (dir, agent, scp)
            } else {
                let dir = probe.temp_path.clone();
                let agent = format!("{dir}\\ksm-agent.exe");
                let scp = format!("{agent}.new");
                (dir, agent, scp)
            }
        }
    }
}

/// Forward-slash form for scp destinations (Windows OpenSSH accepts these).
fn scp_style_path(path: &str) -> String {
    path.replace('\\', "/")
}

/// Backslash form for `cmd.exe` (forward slashes break `mkdir` / `move`).
fn win_cmd_path(path: &str) -> String {
    path.replace('/', "\\")
}

pub fn ensure_agent(host: &str, probe: &ProbeInfo) -> Result<String /* remote agent path */, RemoteError> {
    let local = local_agent_binary(probe.os).ok_or_else(|| RemoteError::Deploy {
        detail: match probe.os {
            RemoteOs::Linux => "ksm-agent binary not found next to kSpaceMonger".into(),
            RemoteOs::Windows => "ksm-agent.exe not found next to kSpaceMonger (build Windows agent)".into(),
        },
    })?;
    let (dir, agent, remote_tmp) = remote_agent_paths(probe);

    let ver = match probe.os {
        RemoteOs::Linux => ssh_base(host)
            .arg(format!(
                "if test -x '{agent}'; then '{agent}' --version 2>/dev/null || true; else echo missing; fi"
            ))
            .output(),
        RemoteOs::Windows => {
            let ag = win_cmd_path(&agent);
            ssh_base(host)
                .arg(format!(
                    r#"cmd.exe /c "if exist {ag} ({ag} --version) else (echo missing)""#
                ))
                .output()
        }
    }
    .map_err(|e| RemoteError::Ssh {
        detail: format!("version check failed: {e}"),
    })?;
    let remote_ver = String::from_utf8_lossy(&ver.stdout)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && *l != "missing")
        .unwrap_or("")
        .to_string();
    if remote_ver == AGENT_VERSION {
        return Ok(agent);
    }

    match probe.os {
        RemoteOs::Linux => {
            let mkdir = ssh_base(host)
                .arg(format!("mkdir -p '{dir}'"))
                .status()
                .map_err(|e| RemoteError::Deploy {
                    detail: format!("mkdir failed: {e}"),
                })?;
            if !mkdir.success() {
                return Err(RemoteError::Deploy {
                    detail: "Not writable — cannot create agent directory".into(),
                });
            }
            let _ = ssh_base(host).arg("pkill -f '[k]sm-agent' || true").status();
        }
        RemoteOs::Windows => {
            let d = win_cmd_path(&dir);
            let mkdir = ssh_base(host)
                .arg(format!(r#"cmd.exe /c "mkdir {d} 2>NUL & if exist {d}\ (echo OK) else (echo FAIL)""#))
                .output()
                .map_err(|e| RemoteError::Deploy {
                    detail: format!("mkdir failed: {e}"),
                })?;
            if !String::from_utf8_lossy(&mkdir.stdout).contains("OK") {
                return Err(RemoteError::Deploy {
                    detail: "Not writable — cannot create agent directory".into(),
                });
            }
            let _ = ssh_base(host)
                .arg(r#"cmd.exe /c "taskkill /F /IM ksm-agent.exe 2>NUL""#)
                .status();
        }
    }

    let scp_dest = format!("{}:{}", host, scp_style_path(&remote_tmp));
    let scp = Command::new("scp")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "-q"])
        .arg(&local)
        .arg(&scp_dest)
        .status()
        .map_err(|e| RemoteError::Deploy {
            detail: format!("scp failed: {e}"),
        })?;
    if !scp.success() {
        return Err(RemoteError::Deploy {
            detail: match probe.os {
                RemoteOs::Linux => "scp of ksm-agent failed".into(),
                RemoteOs::Windows => "scp of ksm-agent.exe failed".into(),
            },
        });
    }

    let install = match probe.os {
        RemoteOs::Linux => ssh_base(host)
            .arg(format!(
                "mv -f '{remote_tmp}' '{agent}' && chmod 755 '{agent}' && '{agent}' --version"
            ))
            .output(),
        RemoteOs::Windows => {
            let tmp = win_cmd_path(&remote_tmp);
            let ag = win_cmd_path(&agent);
            ssh_base(host)
                .arg(format!(
                    r#"cmd.exe /c "move /Y {tmp} {ag} >NUL & {ag} --version""#
                ))
                .output()
        }
    }
    .map_err(|e| RemoteError::Deploy {
        detail: format!("install failed: {e}"),
    })?;
    if !install.status.success() {
        return Err(RemoteError::Deploy {
            detail: String::from_utf8_lossy(&install.stderr).trim().to_string(),
        });
    }
    let got = String::from_utf8_lossy(&install.stdout)
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string();
    if got != AGENT_VERSION {
        return Err(RemoteError::Agent {
            detail: format!("version mismatch after deploy ({got})"),
        });
    }
    Ok(agent)
}

/// Run a full remote scan; updates `progress` and `status` callbacks via shared state.
pub fn scan_remote(
    target: &SshfsTarget,
    remote_path: &str,
    progress: Arc<Progress>,
    status: Arc<std::sync::Mutex<StatusChips>>,
) -> Result<Tree, RemoteError> {
    {
        let mut s = status.lock().unwrap();
        s.set("Retrying", "—", "Idle", "—", format!("probing {}", target.host));
    }
    let probe = ssh_probe(&target.host)?;
    let store = if probe.home_ok { "OK" } else { "Temp" };
    {
        let mut s = status.lock().unwrap();
        s.set(
            "Ready",
            "Deploying",
            "Idle",
            store,
            format!("{} · {}", target.host, probe.label),
        );
    }
    let agent_path = ensure_agent(&target.host, &probe)?;
    {
        let mut s = status.lock().unwrap();
        s.set(
            "Ready",
            "Ready",
            "Running",
            store,
            format!("{} · agent {}", target.host, AGENT_VERSION),
        );
    }

    let mut child = start_agent_session(&target.host, &agent_path, probe.os)?;
    let mut stdin = child.stdin.take().ok_or_else(|| RemoteError::Agent {
        detail: "no stdin".into(),
    })?;
    let stdout = child.stdout.take().ok_or_else(|| RemoteError::Agent {
        detail: "no stdout".into(),
    })?;
    let mut reader = BufReader::new(stdout);

    write_client(&mut stdin, &ClientMsg::Hello)?;
    let hello = read_server(&mut reader)?;
    match hello {
        ServerMsg::HelloOk { version, arch: a, .. } => {
            let mut s = status.lock().unwrap();
            s.set(
                "Ready",
                "Ready",
                "Running",
                store,
                format!("{} · agent {version} ({a})", target.host),
            );
        }
        ServerMsg::Error { msg, .. } => {
            let _ = child.kill();
            return Err(RemoteError::Agent { detail: msg });
        }
        _ => {
            let _ = child.kill();
            return Err(RemoteError::Agent {
                detail: "unexpected hello reply".into(),
            });
        }
    }

    // Defend against any caller still holding sshfs's `/D:/` encoding.
    let scan_path = normalize_sshfs_remote_path(remote_path);
    write_client(
        &mut stdin,
        &ClientMsg::Scan {
            path: scan_path,
        },
    )?;

    let child_id = child.id();
    let progress_watch = progress.clone();
    thread::spawn(move || {
        while !progress_watch.cancel.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));
        }
        let _ = Command::new("kill").arg(child_id.to_string()).status();
    });

    let tree = loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                if progress.cancel.load(Ordering::Relaxed) {
                    return Err(RemoteError::Scan {
                        detail: "cancelled".into(),
                    });
                }
                return Err(RemoteError::Agent {
                    detail: "agent closed connection".into(),
                });
            }
            Ok(_) => {}
            Err(e) => {
                if progress.cancel.load(Ordering::Relaxed) {
                    return Err(RemoteError::Scan {
                        detail: "cancelled".into(),
                    });
                }
                return Err(RemoteError::Agent {
                    detail: format!("read: {e}"),
                });
            }
        }
        if line.trim().is_empty() {
            continue;
        }
        let msg = read_server_line(&line).map_err(|e| RemoteError::Agent { detail: e })?;
        match msg {
            ServerMsg::Progress {
                folders,
                files,
                current,
            } => {
                progress.folders.store(folders, Ordering::Relaxed);
                progress.files.store(files, Ordering::Relaxed);
                if let Ok(mut c) = progress.current.lock() {
                    *c = current;
                }
            }
            done @ ServerMsg::Done { .. } => {
                let mut tree = agent_proto::tree_from_done(done).ok_or_else(|| RemoteError::Scan {
                    detail: "bad done payload".into(),
                })?;
                tree.root_path = local_path_for(target, remote_path);
                if let Some(n) = tree.nodes.get_mut(0) {
                    n.name = tree.root_path.to_string_lossy().to_string();
                }
                break tree;
            }
            ServerMsg::Error { msg, .. } => {
                let _ = child.kill();
                return Err(RemoteError::Scan { detail: msg });
            }
            ServerMsg::HelloOk { .. } => {}
        }
    };

    let _ = write_client(&mut stdin, &ClientMsg::Quit);
    let _ = child.wait();
    {
        let mut s = status.lock().unwrap();
        s.set("Ready", "Ready", "Idle", store, format!("{} · done", target.host));
    }
    Ok(tree)
}

fn start_agent_session(host: &str, agent_path: &str, os: RemoteOs) -> Result<Child, RemoteError> {
    let remote_cmd = match os {
        RemoteOs::Linux => format!("exec '{agent_path}' --agent"),
        // Invoke the .exe directly so stdin/stdout stay on the agent (not a cmd wrapper).
        RemoteOs::Windows => {
            let ag = scp_style_path(agent_path);
            format!("\"{ag}\" --agent")
        }
    };
    ssh_base(host)
        .arg(remote_cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| RemoteError::Agent {
            detail: format!("spawn ssh agent: {e}"),
        })
}

fn write_client(stdin: &mut impl Write, msg: &ClientMsg) -> Result<(), RemoteError> {
    serde_json::to_writer(&mut *stdin, msg).map_err(|e| RemoteError::Agent {
        detail: e.to_string(),
    })?;
    stdin.write_all(b"\n").map_err(|e| RemoteError::Agent {
        detail: e.to_string(),
    })?;
    stdin.flush().map_err(|e| RemoteError::Agent {
        detail: e.to_string(),
    })
}

fn read_server(reader: &mut impl BufRead) -> Result<ServerMsg, RemoteError> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line).map_err(|e| RemoteError::Agent {
            detail: format!("read: {e}"),
        })?;
        if n == 0 {
            return Err(RemoteError::Agent {
                detail: "agent closed connection".into(),
            });
        }
        if line.trim().is_empty() {
            continue;
        }
        return read_server_line(&line).map_err(|e| RemoteError::Agent { detail: e });
    }
}

/// For tests / UI: classify path without SSH.
pub fn is_sshfs_fstype(fstype: &str) -> bool {
    let ft = fstype.to_ascii_lowercase();
    ft.contains("sshfs")
}

fn local_path_for(target: &SshfsTarget, remote_path: &str) -> PathBuf {
    let root = normalize_sshfs_remote_path(&target.remote_root)
        .trim_end_matches(['/', '\\'])
        .replace('\\', "/");
    let remote = normalize_sshfs_remote_path(remote_path)
        .trim_end_matches(['/', '\\'])
        .replace('\\', "/");
    if remote.eq_ignore_ascii_case(&root) || remote_path.is_empty() {
        return target.local_mount.clone();
    }
    let rel = if remote.len() >= root.len() && remote[..root.len()].eq_ignore_ascii_case(&root) {
        remote[root.len()..].trim_start_matches(['/', '\\'])
    } else {
        remote.as_str().trim_start_matches(['/', '\\'])
    };
    if rel.is_empty() {
        target.local_mount.clone()
    } else {
        target.local_mount.join(rel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sshfs_windows_drive_strips_slash() {
        let t = parse_sshfs_line("winhost:/D:/", "/mnt/d", "fuse.sshfs").unwrap();
        assert_eq!(t.host, "winhost");
        assert_eq!(t.remote_root, "D:/");
        let t = parse_sshfs_line("winhost:/C:/Users", "/mnt/c", "fuse.sshfs").unwrap();
        assert_eq!(t.remote_root, "C:/Users");
    }

    #[test]
    fn sshfs_linux_root_unchanged() {
        let t = parse_sshfs_line("linuxhost:/", "/mnt/remote", "fuse.sshfs").unwrap();
        assert_eq!(t.remote_root, "/");
        let t = parse_sshfs_line("linuxhost:/home/user", "/mnt/remote", "fuse.sshfs").unwrap();
        assert_eq!(t.remote_root, "/home/user");
    }
}
