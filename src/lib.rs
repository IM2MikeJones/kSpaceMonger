//! Shared library: local scan, remote agent protocol, SSH orchestration.

pub mod agent_proto;
pub mod scan;

/// SSH deploy/orchestration — Linux GUI host only (not compiled into Windows agent).
#[cfg(not(target_os = "windows"))]
pub mod remote;
