//! Generates the list of built-in label packs from `data/labels/*.toml`, so adding a pack
//! file is enough to ship it.

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/labels");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("data/labels exists")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".toml"))
        .collect();
    names.sort();
    let mut out = String::from("&[\n");
    for n in &names {
        let path = dir.join(n);
        println!("cargo:rerun-if-changed={}", path.display());
        writeln!(
            out,
            "    ({n:?}, include_str!({:?})),",
            path.display().to_string()
        )
        .unwrap();
    }
    out.push(']');
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("builtin_packs.rs");
    std::fs::write(dest, out).unwrap();
}
