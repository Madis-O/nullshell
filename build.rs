// Embeds the icon and version info into nullshell.exe, so Explorer, the
// taskbar and shortcuts show it as a proper app.

fn main() {
    println!("cargo:rerun-if-changed=assets/nullshell.ico");
    println!("cargo:rerun-if-changed=build.rs");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        // Resource id 1; app.rs loads the window icon from the same id.
        res.set_icon_with_id("assets/nullshell.ico", "1");
        res.set("ProductName", "NullShell");
        res.set("FileDescription", "NullShell");
        res.set("InternalName", "nullshell");
        res.set("OriginalFilename", "nullshell.exe");
        res.compile().expect("failed to embed Windows resources");
    }
}
