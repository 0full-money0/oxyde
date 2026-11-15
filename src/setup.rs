//! Installation d'Oxyde sur le PC, sans droits administrateur :
//! l'exe se copie dans %LOCALAPPDATA%\Programs\Oxyde et crée ses raccourcis (menu Démarrer, bureau).

use std::path::PathBuf;
use std::process::Command;

pub fn install_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Programs").join("Oxyde")
}

fn installed_exe() -> PathBuf {
    install_dir().join("Oxyde.exe")
}

fn start_menu_link() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join("Microsoft").join("Windows").join("Start Menu").join("Programs").join("Oxyde.lnk"))
}

fn desktop_link() -> Option<PathBuf> {
    Some(dirs::desktop_dir()?.join("Oxyde.lnk"))
}

/// Oxyde tourne depuis son dossier d'installation.
pub fn is_installed_copy() -> bool {
    std::env::current_exe().ok().and_then(|e| e.canonicalize().ok()) == installed_exe().canonicalize().ok()
}


/// Montrer l'installeur ? Pas pour une copie installée, ni pour une build de développement.
pub fn should_offer() -> bool {
    if !cfg!(windows) || std::env::var("OXYDE_NO_SETUP").is_ok() || std::env::var("OXYDE_TOUR").is_ok() || std::env::var("OXYDE_TAB").is_ok() {
        return false;
    }
    let exe = std::env::current_exe().map(|e| e.display().to_string().to_lowercase()).unwrap_or_default();
    !is_installed_copy() && !exe.contains("target") && !exe.contains("hmc-target")
}

fn ps_quote(p: &std::path::Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "''"))
}

fn powershell(script: &str) -> Result<(), String> {
    let mut c = Command::new("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    let out = c.output().map_err(|e| format!("PowerShell : {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or("échec").to_string())
    }
}

fn shortcut(link: &std::path::Path, target: &std::path::Path) -> Result<(), String> {
    powershell(&format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({});$s.TargetPath={};$s.WorkingDirectory={};$s.IconLocation={};$s.Description='Oxyde - mods et lanceur pour Horizon';$s.Save()",
        ps_quote(link),
        ps_quote(target),
        ps_quote(target.parent().unwrap_or(target)),
        ps_quote(target)
    ))
}

/// Copie l'exe et crée les raccourcis. Renvoie le chemin de la copie installée.
pub fn install(desktop: bool) -> Result<PathBuf, String> {
    let dir = install_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("dossier d'installation : {e}"))?;
    let me = std::env::current_exe().map_err(|e| e.to_string())?;
    let dest = installed_exe();
    std::fs::copy(&me, &dest).map_err(|e| format!("copie d'Oxyde : {e}"))?;
    if let Some(l) = start_menu_link() {
        shortcut(&l, &dest)?;
    }
    if desktop {
        if let Some(l) = desktop_link() {
            shortcut(&l, &dest)?;
        }
    }
    Ok(dest)
}

pub fn launch_installed() {
    let _ = Command::new(installed_exe()).spawn();
}

/// Retire les raccourcis puis le dossier d'Oxyde (après la fermeture de l'app).
pub fn uninstall() -> Result<(), String> {
    for l in [start_menu_link(), desktop_link()].into_iter().flatten() {
        let _ = std::fs::remove_file(l);
    }
    let dir = install_dir();
    let mut c = Command::new("cmd");
    c.args(["/C", &format!("timeout /t 2 /nobreak >nul & rmdir /s /q \"{}\"", dir.display())]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000);
    }
    c.spawn().map_err(|e| e.to_string())?;
    Ok(())
}
