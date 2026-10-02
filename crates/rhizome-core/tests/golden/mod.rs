//! Golden files: checked-in expected output. `RHIZOME_BLESS=1` rewrites them; read every
//! changed golden before committing it, or the test proves nothing.

use std::path::PathBuf;

#[allow(dead_code)]
pub fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

#[allow(dead_code)]
pub fn check(name: &str, actual: &str) {
    check_file(&path(name), actual)
}

#[allow(dead_code)]
pub fn check_file(file: &std::path::Path, actual: &str) {
    let name = file.display();
    if std::env::var_os("RHIZOME_BLESS").is_some() {
        std::fs::write(file, actual).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(file)
        .unwrap_or_else(|_| panic!("no golden {name}; run `just bless` and read it"));
    if expected != actual {
        let diff: Vec<String> = expected
            .lines()
            .zip(actual.lines())
            .enumerate()
            .filter(|(_, (e, a))| e != a)
            .take(10)
            .map(|(i, (e, a))| format!("line {}:\n  - {e}\n  + {a}", i + 1))
            .collect();
        panic!(
            "{name} differs from its golden ({} vs {} lines)\n{}",
            expected.lines().count(),
            actual.lines().count(),
            diff.join("\n")
        );
    }
}
