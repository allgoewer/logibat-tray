fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app.ico");
    if let Err(e) = res.compile() {
        // A missing resource compiler (for example a cross check on Linux)
        // must not block development, but CI must have the icon.
        if std::env::var_os("CI").is_some() {
            panic!("resource compile failed: {e}");
        }
        println!("cargo:warning=resource compile failed: {e}");
    }
}
