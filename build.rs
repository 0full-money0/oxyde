// Ajoute les informations de version Windows (nom, éditeur, version) dans le .exe.
// Un exécutable avec des métadonnées propres est moins souvent signalé par les antivirus.
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "Horizon Mod Converter");
        res.set("FileDescription", "Horizon Mod Converter");
        res.set("CompanyName", "Horizon Mod Converter contributors");
        res.set("LegalCopyright", "MIT License");
        res.set("OriginalFilename", "HorizonModConverter.exe");
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
