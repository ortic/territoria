//! On Windows, the program icon and version go into the .exe.

fn main() {
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=../../packaging/territoria.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/territoria.ico")
            .set("FileDescription", "Territoria – geomarketing and territory planning")
            .set("ProductName", "Territoria");
        if let Err(e) = res.compile() {
            println!("cargo:warning=no icon in the .exe: {e}");
        }
    }
}
