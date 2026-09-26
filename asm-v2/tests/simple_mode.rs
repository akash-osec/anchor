use std::{path::PathBuf, process::Command};

#[test]
fn documented_simple_mode_setup_compiles() {
    let manifest =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/simple-mode/Cargo.toml");
    let lockfile = manifest.with_file_name("Cargo.lock");
    let target_dir =
        std::env::temp_dir().join(format!("anchor-asm-v2-simple-mode-{}", std::process::id()));

    let output = Command::new(std::env::var_os("CARGO").expect("CARGO is set"))
        .arg("check")
        .arg("--manifest-path")
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", &target_dir)
        .output()
        .expect("run cargo check for the simple-mode fixture");

    std::fs::remove_file(lockfile).ok();
    std::fs::remove_dir_all(&target_dir).ok();

    if !output.status.success() {
        panic!(
            "simple-mode fixture failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
