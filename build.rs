//! Zasoby Windows: ikona i metadane pliku wykonywalnego.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "AeroFlow");
        res.set("FileDescription", "AeroFlow — tunel aerodynamiczny 2D");
        res.set("CompanyName", "BlackSingularities");
        res.set("LegalCopyright", "Licencja MIT");
        if let Err(e) = res.compile() {
            println!("cargo:warning=Nie można dołączyć zasobów Windows: {e}");
        }
    }
}
