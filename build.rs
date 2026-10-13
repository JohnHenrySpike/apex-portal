//! Builds every `locales/*.json` into the app (see `src/lang.rs`).

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=locales");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("locales/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let mut out = String::from("const BUILT_IN: &[(&str, &str)] = &[\n");
    for p in files {
        let code = p.file_stem().and_then(|s| s.to_str()).expect("file name");
        out += &format!(
            "    ({code:?}, include_str!({:?})),\n",
            p.display().to_string()
        );
    }
    out += "];\n";
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("locales.rs");
    std::fs::write(dest, out).unwrap();
}
