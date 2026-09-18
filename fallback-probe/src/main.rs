use std::path::{Path, PathBuf};
use std::time::Instant;

fn collect(root: &Path) -> Vec<PathBuf> {
    let subdirs = ["", "autogen", "autogen/metadata", "autogen/enums", "math-docs"];
    let mut files = Vec::new();
    for sub in subdirs {
        let dir = if sub.is_empty() { root.to_path_buf() } else { root.join(sub) };
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut here: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "lua")).collect();
            here.sort();
            files.extend(here);
        }
    }
    files
}

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).unwrap());
    let files = collect(&root);
    let sources: Vec<(PathBuf, String)> = files.iter()
        .map(|p| (p.clone(), std::fs::read_to_string(p).unwrap_or_default())).collect();
    let mut ok = 0usize;
    let mut failed: Vec<(String, usize)> = Vec::new();
    let started = Instant::now();
    for (path, src) in &sources {
        let result = full_moon::parse_fallible(src, full_moon::LuaVersion::lua51());
        let errs = result.errors().len();
        if errs == 0 { ok += 1; } else {
            failed.push((path.file_name().unwrap().to_string_lossy().into_owned(), errs));
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    println!("{} files, {:.3} s, {} clean, {} with errors", files.len(), elapsed, ok, failed.len());
    for (name, n) in failed.iter().take(10) { println!("   {name}: {n} errors"); }
    if failed.len() > 10 { println!("   ... {} more", failed.len() - 10); }
}
