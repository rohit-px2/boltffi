use std::path::PathBuf;
use std::process::Command;

#[test]
fn transparent_tuple_newtype_compiles_through_data() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tuple_newtype/Cargo.toml");
    let target = std::env::temp_dir().join(format!("boltffi-tuple-newtype-{}", std::process::id()));
    if target.exists() {
        std::fs::remove_dir_all(&target).expect("remove stale fixture target directory");
    }
    std::fs::create_dir(&target).expect("create fixture target directory");

    let output = Command::new(env!("CARGO"))
        .args(["check", "--locked"])
        .arg("--manifest-path")
        .arg(&fixture)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .expect("fixture cargo check starts");
    let stderr = String::from_utf8_lossy(&output.stderr);
    std::fs::remove_dir_all(&target).expect("remove fixture target directory");

    assert!(
        output.status.success(),
        "transparent tuple newtype fixture failed\nstdout\n{}\nstderr\n{stderr}",
        String::from_utf8_lossy(&output.stdout)
    );
}
