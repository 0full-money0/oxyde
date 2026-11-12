// Ajoute les informations de version Windows (nom, éditeur, version) dans le .exe.
// Un exécutable avec des métadonnées propres est moins souvent signalé par les antivirus.
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "Oxyde");
        res.set("FileDescription", "Oxyde - mods et lanceur pour Horizon");
        res.set("CompanyName", "full_money");
        res.set("LegalCopyright", "Copyright (c) 2026 full_money - MIT License");
        res.set("OriginalFilename", "Oxyde.exe");
        if std::path::Path::new("assets/icon.ico").exists() {
            res.set_icon("assets/icon.ico");
        }
        if let Err(e) = res.compile() {
            println!("cargo:warning=winresource: {e}");
        }
    }
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-env-changed=HMC_REPO");
}
