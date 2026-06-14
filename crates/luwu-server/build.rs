// build.rs — compile the Ink TUI into a standalone binary via `bun build --compile`,
// then embed it into the Rust binary at compile time via include_bytes!.
// If bun is not available or the build fails, we silently fall back to headless-only mode.
use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(tui_embedded)");
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let ui_dir = manifest_dir.join("../../ui");
    let dist_dir = ui_dir.join("dist");
    let tui_out = dist_dir.join("luwu-tui");

    // Locate bun
    let bun = match find_bun() {
        Some(b) => b,
        None => {
            println!("cargo:warning=bun not found — building headless-only (no TUI embedded)");
            return;
        }
    };

    // bun install if node_modules missing
    let node_modules = ui_dir.join("node_modules");
    if !node_modules.exists() {
        match Command::new(&bun).arg("install").current_dir(&ui_dir).status() {
            Ok(s) if s.success() => {}
            _ => {
                println!("cargo:warning=bun install failed — building headless-only");
                return;
            }
        }
    }

    // Ensure dist/ exists
    let _ = std::fs::create_dir_all(&dist_dir);

    // bun build --compile → standalone executable with bun runtime embedded
    let status = Command::new(&bun)
        .args([
            "build", "src/index.tsx",
            "--compile",
            "--outfile", "dist/luwu-tui",
            "--external", "react-devtools-core",
        ])
        .current_dir(&ui_dir)
        .status();

    match status {
        Ok(s) if s.success() => {
            // Verify the output exists
            if tui_out.exists() {
                println!("cargo:rustc-cfg=tui_embedded");
                println!("cargo:rerun-if-changed=../../ui/src");
            } else {
                println!("cargo:warning=bun build reported success but output not found");
            }
        }
        _ => {
            println!("cargo:warning=bun build --compile failed — building headless-only");
        }
    }
}

fn find_bun() -> Option<String> {
    // Check PATH for bun
    let out = Command::new("which").arg("bun").output().ok()?;
    if !out.status.success() { return None; }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}
