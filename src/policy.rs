//! Owner policy for native execution (docs/08 8.2, 8.4). Without a policy only `emulated` runs.
//! A native executor runs a subject only if a `[[native_allow]]` entry names its digest, or a
//! `[[native_trial_zones]]` entry covers every executed file and every isolation capability the
//! zone requires was confirmed by this host's probe. Otherwise the claims are NOT_EVALUATED with
//! NATIVE_NOT_PERMITTED; nothing falls back to another executor.

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
struct PolicyFile {
    #[serde(default)]
    native_trial_zones: Vec<ZoneFile>,
    #[serde(default)]
    native_allow: Vec<AllowFile>,
    /// Accept self-checks whose only checker is the same version (docs/06 6.9). Default false.
    #[serde(default)]
    allow_self_accept: bool,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct ZoneFile {
    id: String,
    artifact_dir: String,
    executors: Vec<String>,
    targets: Vec<String>,
    require_isolation: Vec<String>,
    max_wall_ms_per_case: Option<u64>,
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct AllowFile {
    digest: String,
    executors: Vec<String>,
    /// Isolation to require and apply (defaults to none beyond process separation).
    #[serde(default)]
    require_isolation: Vec<String>,
}

pub struct Zone {
    pub id: String,
    pub dir: PathBuf,
    pub executors: Vec<String>,
    pub targets: Vec<String>,
    pub require_isolation: Vec<String>,
    pub max_wall_ms_per_case: Option<u64>,
}

pub struct Policy {
    pub path: PathBuf,
    pub digest: String,
    pub zones: Vec<Zone>,
    allow: Vec<AllowFile>,
    pub allow_self_accept: bool,
}

pub const CAPABILITIES: [&str; 7] = [
    "process_isolation",
    "resource_limits",
    "filesystem_restriction",
    "network_restriction",
    "descendant_process_control",
    "credential_isolation",
    "artifact_immutability",
];

impl Policy {
    pub fn load(path: &Path) -> Result<Policy> {
        let text = std::fs::read_to_string(path).with_context(|| format!("policy {}", path.display()))?;
        let f: PolicyFile = toml::from_str(&text).map_err(|e| anyhow::anyhow!("POLICY_ERROR: {}: {e}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let mut zones = Vec::new();
        for z in f.native_trial_zones {
            for c in &z.require_isolation {
                if !CAPABILITIES.contains(&c.as_str()) {
                    bail!("POLICY_ERROR: zone {}: unknown isolation capability `{c}`", z.id);
                }
            }
            for e in &z.executors {
                if !["native-routine", "native-process"].contains(&e.as_str()) {
                    bail!("POLICY_ERROR: zone {}: executor `{e}` cannot be permitted by a zone", z.id);
                }
            }
            zones.push(Zone { id: z.id, dir: dir.join(z.artifact_dir), executors: z.executors, targets: z.targets, require_isolation: z.require_isolation, max_wall_ms_per_case: z.max_wall_ms_per_case });
        }
        Ok(Policy { path: path.to_path_buf(), digest: crate::spec::sha256_hex(text.as_bytes()), zones, allow: f.native_allow, allow_self_accept: f.allow_self_accept })
    }

    /// `--policy`, else `policy.toml` next to the store directory (docs/06 6.11), else none.
    pub fn find(explicit: Option<&Path>, store_root: &Path) -> Result<Option<Policy>> {
        if let Some(p) = explicit {
            return Policy::load(p).map(Some);
        }
        let parent = store_root.parent().map(Path::to_path_buf).unwrap_or_default();
        let p = if parent.as_os_str().is_empty() { PathBuf::from("policy.toml") } else { parent.join("policy.toml") };
        if p.exists() { Policy::load(&p).map(Some) } else { Ok(None) }
    }
}

pub struct Permit {
    /// `zone:<id>` or `digest`.
    pub basis: String,
    pub isolation_required: Vec<String>,
    pub max_wall_ms_per_case: Option<u64>,
}

/// Resolve `.` and `..` by text only (no symbolic links are followed).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

fn inside(file: &Path, dir: &Path) -> Result<(), String> {
    let meta = std::fs::symlink_metadata(file).map_err(|e| format!("{}: {e}", file.display()))?;
    if meta.file_type().is_symlink() {
        return Err(format!("{} is a symbolic link (not followed)", file.display()));
    }
    let abs_file = std::path::absolute(file).map_err(|e| e.to_string())?;
    let abs_dir = std::path::absolute(dir).map_err(|e| e.to_string())?;
    let canon_file = std::fs::canonicalize(file).map_err(|e| e.to_string())?;
    let canon_dir = std::fs::canonicalize(dir).map_err(|e| format!("zone directory {}: {e}", dir.display()))?;
    let lexical_ok = normalize(&abs_file).starts_with(normalize(&abs_dir));
    if !(lexical_ok && canon_file.starts_with(&canon_dir)) {
        return Err(format!("{} is outside {} (or reaches it through a symbolic link)", file.display(), dir.display()));
    }
    Ok(())
}

/// Decide whether `executor` may run the subject. `files` are every executed file, `digest` the
/// subject digest shown in the run scope, `probe` this host's HostProbe.
pub fn permit(policy: Option<&Policy>, executor: &str, target: &str, files: &[PathBuf], digest: &str, probe: &serde_json::Value) -> Result<Permit, String> {
    let Some(p) = policy else {
        return Err("NATIVE_NOT_PERMITTED: no policy.toml (only `emulated` runs by default; docs/08 8.2)".into());
    };
    let confirmed = |c: &str| probe["capabilities"][c]["confirmed"].as_bool().unwrap_or(false);
    for a in &p.allow {
        if a.digest == digest && a.executors.iter().any(|e| e == executor) {
            let missing: Vec<&String> = a.require_isolation.iter().filter(|c| !confirmed(c)).collect();
            if !missing.is_empty() {
                return Err(format!("NATIVE_NOT_PERMITTED: digest allowed, but isolation not confirmed on this host: {missing:?}"));
            }
            return Ok(Permit { basis: "digest".into(), isolation_required: a.require_isolation.clone(), max_wall_ms_per_case: None });
        }
    }
    let mut why = Vec::new();
    for z in &p.zones {
        if !z.executors.iter().any(|e| e == executor) {
            why.push(format!("zone {}: executor {executor} not listed", z.id));
            continue;
        }
        if !z.targets.iter().any(|t| t == target) {
            why.push(format!("zone {}: target {target} not listed", z.id));
            continue;
        }
        if let Some(e) = files.iter().find_map(|f| inside(f, &z.dir).err()) {
            why.push(format!("zone {}: {e}", z.id));
            continue;
        }
        let missing: Vec<&String> = z.require_isolation.iter().filter(|c| !confirmed(c)).collect();
        if !missing.is_empty() {
            why.push(format!("zone {}: isolation not confirmed on this host: {missing:?}", z.id));
            continue;
        }
        return Ok(Permit { basis: format!("zone:{}", z.id), isolation_required: z.require_isolation.clone(), max_wall_ms_per_case: z.max_wall_ms_per_case });
    }
    if why.is_empty() {
        why.push("no trial zone and no digest entry".into());
    }
    Err(format!("NATIVE_NOT_PERMITTED: {}", why.join("; ")))
}
