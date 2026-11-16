fn main() {
    // Embed the icon into the Windows .exe (if assets/icon.ico exists)
    #[cfg(windows)]
    {
        if std::path::Path::new("assets/icon.ico").exists() {
            let mut res = winresource::WindowsResource::new();
            res.set_icon("assets/icon.ico");
            res.set("ProductName", "LyricsOSC");
            res.set("LegalCopyright", "GPL-3.0-or-later");
            let _ = res.compile();
        }
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=locales");
}
