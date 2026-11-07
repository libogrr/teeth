//! Embeds the icon and the version information in the Windows executable, so Explorer, the
//! Start menu and the installer show them.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon/teeth.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/icon/teeth.ico");
    res.set("ProductName", "Teeth");
    res.set("FileDescription", "Teeth");
    res.set("OriginalFilename", "Teeth.exe");
    // Only the app gets them: `fidelity` and the test binaries are not Teeth.
    if let Err(e) = res.compile_for("teeth") {
        panic!("no se pudo incrustar el icono: {e}");
    }
}
