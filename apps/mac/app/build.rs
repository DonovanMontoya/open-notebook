fn main() {
    println!("cargo:rerun-if-changed=native/PDFReader.swift");
    println!("cargo:rerun-if-changed=native/FigureHTML.swift");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").unwrap().as_str() {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => panic!("unsupported Mac architecture: {other}"),
    };
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("libNotebookPDF.dylib");
    let result = std::process::Command::new("xcrun")
        .args(["swiftc", "-emit-library", "-O", "-target", &format!("{arch}-apple-macos13.0"), "native/PDFReader.swift", "native/FigureHTML.swift", "-o"])
        .arg(&output)
        .status()
        .expect("Swift compiler for the native PDF reader");
    assert!(result.success(), "native PDF reader build failed");
}
