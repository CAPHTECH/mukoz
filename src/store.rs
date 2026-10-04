//! Evidence store under `.mukoz/` (docs/06 6.11) and regression cases (docs/04 4.8).

use crate::expr::Value;
use crate::plan::{Case, Origin};
use crate::spec::{Contract, sha256_hex};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub const REGRESSION_LIMIT: usize = 1024;

pub struct Store {
    pub root: PathBuf,
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect()
}

impl Store {
    pub fn open(root: &Path) -> Result<Store> {
        for d in ["items", "objects", "regressions"] {
            std::fs::create_dir_all(root.join(d)).with_context(|| format!("creating store {}", root.display()))?;
        }
        Ok(Store { root: root.to_path_buf() })
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
        Ok(())
    }

    pub fn put_object(&self, bytes: &[u8]) -> Result<String> {
        let d = sha256_hex(bytes);
        let p = self.root.join("objects").join(&d);
        if !p.exists() {
            self.write_atomic(&p, bytes)?;
        }
        Ok(d)
    }

    pub fn put_item(&self, id: &str, v: &serde_json::Value) -> Result<()> {
        let p = self.root.join("items").join(format!("{}.json", sanitize(id)));
        self.write_atomic(&p, &serde_json::to_vec_pretty(v)?)
    }

    pub fn get_item(&self, id: &str) -> Result<serde_json::Value> {
        let p = self.root.join("items").join(format!("{}.json", sanitize(id)));
        let text = std::fs::read(&p).with_context(|| format!("no item `{id}` in {}", self.root.display()))?;
        Ok(serde_json::from_slice(&text)?)
    }

    fn regression_dir(&self, contract: &Contract, target: &str) -> PathBuf {
        self.root.join("regressions").join(sanitize(&contract.id)).join(&contract.digest[..16]).join(sanitize(target))
    }

    pub fn add_regression(&self, contract: &Contract, target: &str, case: &Case, source: &str) -> Result<bool> {
        let dir = self.regression_dir(contract, target);
        std::fs::create_dir_all(&dir)?;
        let values: serde_json::Map<String, serde_json::Value> = case.values.iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
        let key = sha256_hex(&serde_json::to_vec(&values)?);
        let p = dir.join(format!("{}.json", &key[..16]));
        if p.exists() {
            return Ok(false);
        }
        let existing = std::fs::read_dir(&dir)?.count();
        if existing >= REGRESSION_LIMIT {
            bail!("REGRESSION_LIMIT_EXCEEDED: {existing} regression cases for {} / {target}; prune explicitly", contract.id);
        }
        let v = serde_json::json!({
            "contract_id": contract.id,
            "contract_digest": contract.digest,
            "target": target,
            "values": values,
            "filler_seed": format!("0x{:016x}", case.filler_seed),
            "source_counterexample": source,
        });
        self.write_atomic(&p, &serde_json::to_vec_pretty(&v)?)?;
        Ok(true)
    }

    /// Regression cases for this contract digest and target, plus the number of
    /// stored cases that do not apply (other digests of the same contract id,
    /// or values that no longer type-check).
    pub fn load_regressions(&self, contract: &Contract, target: &str) -> Result<(Vec<(String, Case)>, usize)> {
        let mut out = Vec::new();
        let mut na = 0;
        let base = self.root.join("regressions").join(sanitize(&contract.id));
        if !base.exists() {
            return Ok((out, 0));
        }
        let mut digests: Vec<_> = std::fs::read_dir(&base)?.filter_map(|e| e.ok()).collect();
        digests.sort_by_key(|e| e.file_name());
        for d in digests {
            let tdir = d.path().join(sanitize(target));
            if !tdir.exists() {
                continue;
            }
            let mut files: Vec<_> = std::fs::read_dir(&tdir)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            files.sort();
            let current = d.file_name().to_string_lossy() == contract.digest[..16];
            for f in files {
                if !current {
                    na += 1;
                    continue;
                }
                let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&f)?)?;
                match parse_case(contract, &v) {
                    Some(c) => out.push((f.file_stem().unwrap().to_string_lossy().to_string(), c)),
                    None => na += 1,
                }
            }
        }
        Ok((out, na))
    }

    pub fn list_regressions(&self, contract: &Contract, target: &str) -> Result<Vec<serde_json::Value>> {
        let dir = self.regression_dir(contract, target);
        let mut out = Vec::new();
        if dir.exists() {
            let mut files: Vec<_> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            files.sort();
            for f in files {
                let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&f)?)?;
                v["id"] = serde_json::json!(f.file_stem().unwrap().to_string_lossy());
                out.push(v);
            }
        }
        Ok(out)
    }

    pub fn prune_regression(&self, contract: &Contract, target: &str, id: &str) -> Result<()> {
        let p = self.regression_dir(contract, target).join(format!("{}.json", sanitize(id)));
        if !p.exists() {
            bail!("no regression case `{id}`");
        }
        let removed: serde_json::Value = serde_json::from_slice(&std::fs::read(&p)?)?;
        std::fs::remove_file(&p)?;
        let log = self.root.join("regressions").join("prune-log.jsonl");
        let mut line = serde_json::to_string(&serde_json::json!({ "pruned": id, "case": removed }))?;
        line.push('\n');
        use std::io::Write;
        std::fs::OpenOptions::new().create(true).append(true).open(log)?.write_all(line.as_bytes())?;
        Ok(())
    }
}

fn parse_case(contract: &Contract, v: &serde_json::Value) -> Option<Case> {
    let vals = v.get("values")?.as_object()?;
    let mut env = crate::expr::ValEnv::new();
    for (prefix, m) in [("input", &contract.inputs), ("before", &contract.state)] {
        for (name, vt) in m {
            let key = format!("{prefix}.{name}");
            let val = Value::from_json(vt.ty, vals.get(&key)?)?;
            if let Value::Bytes(b) = &val {
                if b.len() as u64 > vt.max_len {
                    return None;
                }
            }
            env.insert(key, val);
        }
    }
    if vals.len() != env.len() {
        return None;
    }
    let seed = u64::from_str_radix(v.get("filler_seed")?.as_str()?.strip_prefix("0x")?, 16).ok()?;
    Some(Case { id: String::new(), origin: Origin::Regression, values: env, filler_seed: seed })
}
