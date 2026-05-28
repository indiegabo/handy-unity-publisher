//! Embeds Windows metadata so the bundled runtime is identifiable as part of HGP.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource
            .set("FileDescription", "hgp-runtime")
            .set("ProductName", "HGP")
            .set("InternalName", "hgp-runtime.exe")
            .set("OriginalFilename", "hgp-runtime.exe")
            .compile()
            .expect("runtime Windows metadata should compile");
    }
}
