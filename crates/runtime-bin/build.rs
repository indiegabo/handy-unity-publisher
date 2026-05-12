//! Embeds Windows metadata so the bundled runtime is identifiable as part of HUP.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set("FileDescription", "hup-runtime")
            .set("ProductName", "HUP")
            .set("InternalName", "hup-runtime.exe")
            .set("OriginalFilename", "hup-runtime.exe")
            .compile()
            .expect("runtime Windows metadata should compile");
    }
}