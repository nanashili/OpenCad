use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!(
            "opencad-cli-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            }
        ));
        fs::create_dir(&directory).unwrap();
        Self(directory)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dxf_commands_preserve_3d_geometry_and_validate_before_writing() {
    let scratch = Scratch::new();
    let binary = env!("CARGO_BIN_EXE_opencad");
    let output = scratch.0.join("line.dxf");
    let result = Command::new(binary)
        .arg("line3")
        .arg(&output)
        .args(["1", "2", "3", "4", "6", "15"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output).unwrap(),
        include_str!("fixtures/line3.dxf")
    );

    let input =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reordered-lines.dxf");
    let result = Command::new(binary)
        .arg("dxf-copy")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let lines = opencad::read_dxf(&fs::read_to_string(&output).unwrap()).unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[0].end(),
        opencad::Point3::new(4.0, 6.0, 15.0).unwrap()
    );

    let unsupported = scratch.0.join("unsupported.dxf");
    fs::write(
        &unsupported,
        include_str!("fixtures/line3.dxf").replace("LINE", "SPLINE"),
    )
    .unwrap();
    fs::write(&output, "existing drawing").unwrap();
    let result = Command::new(binary)
        .arg("dxf-copy")
        .arg(&unsupported)
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("unsupported entity SPLINE"));
    assert_eq!(fs::read_to_string(&output).unwrap(), "existing drawing");

    let result = Command::new(binary)
        .arg("line3")
        .arg(&output)
        .args(["1", "2", "NaN", "4", "6", "15"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("coordinates must be finite"));
    assert_eq!(fs::read_to_string(&output).unwrap(), "existing drawing");
}

#[test]
fn source_export_retains_document_records_during_exchange_and_editing() {
    let scratch = Scratch::new();
    let binary = env!("CARGO_BIN_EXE_opencad");
    let input =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/autocad-r12-lines.dxf");
    let output = scratch.0.join("source-copy.dxf");
    let result = Command::new(binary)
        .arg("dxf-copy")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(&output).unwrap(), fs::read(&input).unwrap());

    let result = Command::new(binary)
        .arg("dxf-set-line")
        .arg(&input)
        .arg(&output)
        .args(["1", "2", "3", "4", "8", "11", "16"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original = fs::read_to_string(&input).unwrap();
    let expected = original.replacen(
        " 10\n1.0\n 20\n2.0\n 30\n3.0\n 11\n4.0\n 21\n6.0\n 31\n15.0\n",
        " 10\n2\n 20\n3\n 30\n4\n 11\n8\n 21\n11\n 31\n16\n",
        1,
    );
    assert_ne!(expected, original);
    assert_eq!(fs::read_to_string(&output).unwrap(), expected);
    let lines = opencad::read_dxf(&expected).unwrap();
    assert_eq!(
        lines[0].start(),
        opencad::Point3::new(2.0, 3.0, 4.0).unwrap()
    );
    assert_eq!(
        lines[0].end(),
        opencad::Point3::new(8.0, 11.0, 16.0).unwrap()
    );
    assert_eq!(lines[1].start(), lines[1].end());

    for invalid in ["0", "3"] {
        let result = Command::new(binary)
            .arg("dxf-set-line")
            .arg(&input)
            .arg(&output)
            .args([invalid, "2", "3", "4", "8", "11", "16"])
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert_eq!(fs::read_to_string(&output).unwrap(), expected);
    }
}
