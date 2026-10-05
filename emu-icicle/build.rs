//! Embeds the SLEIGH processor specifications under `sleigh/` into the binary.

use std::path::Path;

fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            walk(&p, root, out);
        } else if p.parent() != Some(root) {
            out.push(p.strip_prefix(root).unwrap().to_str().unwrap().replace('\\', "/"));
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("sleigh");
    println!("cargo:rerun-if-changed=sleigh");
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    let mut src = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for f in files {
        src.push_str(&format!("    ({f:?}, include_bytes!({:?})),\n", root.join(&f)));
    }
    src.push_str("];\n");
    std::fs::write(Path::new(&std::env::var("OUT_DIR").unwrap()).join("sleigh_files.rs"), src).unwrap();
}
