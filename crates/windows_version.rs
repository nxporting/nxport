// Build-script helper (include!d by the engine's and the builder's build.rs): a Windows version resource for the
// crate's programs, so the files say what they are (product, description, version, origin) in Explorer's properties
// and to anything that reads them, as installed software does. Compiled with the Windows SDK's resource compiler
// (rc.exe, from the RC variable or the newest SDK under Windows Kits) and linked into the binaries. Skipped, with a
// warning, when there is no SDK.

/// Writes and compiles the resource for the crate's binaries, described as `description`.
#[allow(dead_code)]
fn windows_version_resource(description: &str) {
    use std::path::PathBuf;
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let mut parts: Vec<u16> = version.split(['.', '-', '+']).take(3).map(|p| p.parse().unwrap_or(0)).collect();
    parts.resize(4, 0);
    let numeric = parts.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
    let name = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    let rc = format!(
        // FILEOS VOS_NT_WINDOWS32 (0x40004) and FILETYPE VFT_APP (1) as numbers: winver.h is only on the include path
        // inside a Visual Studio prompt.
        r#"1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "The nxport contributors\0"
      VALUE "FileDescription", "{description}\0"
      VALUE "FileVersion", "{version}\0"
      VALUE "InternalName", "{name}\0"
      VALUE "LegalCopyright", "MIT License, the nxport contributors\0"
      VALUE "OriginalFilename", "{name}.exe\0"
      VALUE "ProductName", "nxport\0"
      VALUE "ProductVersion", "{version}\0"
      VALUE "Comments", "https://github.com/nxporting/nxport\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let (rc_path, res_path) = (out.join("version.rc"), out.join("version.res"));
    std::fs::write(&rc_path, rc).unwrap();
    let Some(compiler) = resource_compiler() else {
        println!("cargo::warning=no rc.exe (Windows SDK): the programs are built without version information");
        return;
    };
    let status = std::process::Command::new(&compiler).arg("/nologo").arg("/fo").arg(&res_path).arg(&rc_path).status();
    if !status.is_ok_and(|s| s.success()) {
        println!("cargo::warning={}: compiling the version resource failed", compiler.display());
        return;
    }
    println!("cargo::rustc-link-arg-bins={}", res_path.display());
    println!("cargo::rerun-if-env-changed=RC");
}

/// rc.exe: the RC variable, else the newest Windows 10/11 SDK's for the host.
#[allow(dead_code)]
fn resource_compiler() -> Option<std::path::PathBuf> {
    if let Some(rc) = std::env::var_os("RC").map(std::path::PathBuf::from).filter(|p| p.is_file()) {
        return Some(rc);
    }
    let root = std::path::PathBuf::from(std::env::var_os("ProgramFiles(x86)")?).join("Windows Kits").join("10").join("bin");
    let host = if cfg!(target_arch = "aarch64") { "arm64" } else { "x64" };
    let mut versions: Vec<_> = std::fs::read_dir(&root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(host).join("rc.exe").is_file())
        .collect();
    // Version folders (10.0.19041.0, ...): compare numerically.
    versions.sort_by_key(|p| {
        p.file_name().and_then(|n| n.to_str()).unwrap_or("").split('.').map(|x| x.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>()
    });
    versions.pop().map(|p| p.join(host).join("rc.exe"))
}
