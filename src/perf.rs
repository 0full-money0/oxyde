//! Optimisation du jeu avant le lancement, et surveillance de la partie.
//!
//! Sur une puce graphique intégrée (Intel UHD…), une distance de rendu trop grande remplit la
//! mémoire vidéo et Horizon plante (« wgpu error: Out of Memory »). Oxyde règle donc le profil
//! selon le PC avant chaque lancement, et lit le journal du jeu après coup pour expliquer un plantage.

use std::path::{Path, PathBuf};

/// Ce que le PC peut encaisser.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tier {
    /// Puce graphique intégrée ou peu de mémoire vidéo.
    Light,
    /// Vraie carte graphique.
    Strong,
}

#[derive(Clone, Debug)]
pub struct Machine {
    pub tier: Tier,
    pub gpu: String,
}

/// Lit la carte graphique : d'abord dans le journal de Horizon (instantané), sinon via Windows.
pub fn detect(game_dir: &Path) -> Machine {
    if let Some(m) = from_log(&game_dir.join("logs").join("latest.log")) {
        return m;
    }
    from_windows().unwrap_or(Machine { tier: Tier::Light, gpu: "carte graphique inconnue".into() })
}

fn from_log(path: &Path) -> Option<Machine> {
    let text = std::fs::read_to_string(path).ok()?;
    // « Horizon renderer: Intel(R) UHD Graphics (Vulkan, IntegratedGpu) »
    let line = text.lines().find(|l| l.contains("Horizon renderer:") && l.contains("Gpu)"))?;
    let after = line.split("Horizon renderer:").nth(1)?.trim();
    let gpu = after.split(" (").next().unwrap_or(after).to_string();
    let integrated = after.contains("IntegratedGpu") || after.contains("Cpu)");
    Some(Machine { tier: if integrated { Tier::Light } else { Tier::Strong }, gpu })
}

#[cfg(windows)]
fn from_windows() -> Option<Machine> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-CimInstance Win32_VideoController | Where-Object { $_.Name -notmatch 'Virtual|Basic|Parsec|Remote' } | ForEach-Object { $_.Name }",
        ])
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut best: Option<Machine> = None;
    for l in text.lines() {
        let name = l.trim().to_string();
                if name.is_empty() {
            continue;
        }
        let low = name.to_lowercase();
        let integrated = low.contains("uhd") || low.contains("hd graphics") || low.contains("iris") || (low.contains("radeon") && low.contains("graphics") && !low.contains("rx"));
        let m = Machine { tier: if integrated { Tier::Light } else { Tier::Strong }, gpu: name };
        // une vraie carte graphique l'emporte sur la puce intégrée
        if best.as_ref().is_none_or(|b| b.tier == Tier::Light && m.tier == Tier::Strong) {
            best = Some(m);
        }
    }
    best
}

#[cfg(not(windows))]
fn from_windows() -> Option<Machine> {
    None
}

/// Une ligne `clé:valeur` d'un fichier d'options.
fn get(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| l.strip_prefix(key).and_then(|r| r.strip_prefix(':')).map(|v| v.trim().to_string()))
}

fn set(text: &mut String, key: &str, value: &str) {
    let mut found = false;
    let mut out = String::with_capacity(text.len() + 32);
    for l in text.lines() {
        if l.strip_prefix(key).is_some_and(|r| r.starts_with(':')) {
            out.push_str(&format!("{key}:{value}"));
            found = true;
        } else {
            out.push_str(l);
        }
        out.push('\n');
    }
    if !found {
        out.push_str(&format!("{key}:{value}\n"));
    }
    *text = out;
}

/// Plafonne une valeur numérique ; renvoie le changement fait (« 64 → 10 »).
fn cap(text: &mut String, key: &str, max: i64) -> Option<(i64, i64)> {
    let v: i64 = get(text, key)?.trim_matches('"').parse().ok()?;
    if v > max {
        set(text, key, &max.to_string());
        Some((v, max))
    } else {
        None
    }
}

/// Règle le profil pour ce PC. Renvoie la liste des changements, lisible par un humain.
pub fn optimize(game_dir: &Path, m: &Machine) -> Vec<String> {
    let mut changes = vec![];
    let (render, sim, clouds) = match m.tier {
        Tier::Light => (10, 6, 64),
        Tier::Strong => (24, 12, 128),
    };
    let opts = game_dir.join("options.txt");
    if let Ok(mut t) = std::fs::read_to_string(&opts) {
        let before = t.clone();
        if let Some((a, b)) = cap(&mut t, "renderDistance", render) {
            changes.push(format!("distance de rendu {a} → {b} chunks"));
        }
        if let Some((a, b)) = cap(&mut t, "simulationDistance", sim) {
            changes.push(format!("distance de simulation {a} → {b}"));
        }
        if cap(&mut t, "cloudRange", clouds).is_some() {
            changes.push("nuages plus proches".into());
        }
        if m.tier == Tier::Light {
            if get(&t, "maxFps").and_then(|v| v.parse::<i64>().ok()).is_some_and(|v| v > 60 || v == 0) {
                set(&mut t, "maxFps", "60");
                changes.push("images par seconde plafonnées à 60".into());
            }
            if cap(&mut t, "mipmapLevels", 2).is_some() {
                changes.push("mipmaps 4 → 2".into());
            }
        }
        if t != before {
            let _ = std::fs::write(&opts, t);
        }
    }
    let gfx = game_dir.join("horizon-graphics.txt");
    if let Ok(mut t) = std::fs::read_to_string(&gfx) {
        let before = t.clone();
        // masquer les grottes invisibles : plus d'images, moins de mémoire, aucun défaut visible
        if get(&t, "caveCulling").as_deref() == Some("false") {
            set(&mut t, "caveCulling", "true");
            changes.push("grottes cachées non dessinées".into());
        }
        if m.tier == Tier::Light && get(&t, "framePacing").as_deref() != Some("efficient") {
            set(&mut t, "framePacing", "efficient");
            changes.push("rythme d'images économe".into());
        }
        if t != before {
            let _ = std::fs::write(&gfx, t);
        }
    }
    changes
}

/// Le jeu tourne-t-il ? (processus horizon-client.exe)
pub fn game_running() -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq horizon-client.exe", "/NH", "/FO", "CSV"])
            .creation_flags(0x0800_0000)
            .output();
        return out.map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains("horizon-client.exe")).unwrap_or(false);
    }
    #[allow(unreachable_code)]
    false
}

/// Pourquoi la dernière partie s'est arrêtée, si elle a planté.
pub fn last_crash(game_dir: &Path, since: std::time::SystemTime) -> Option<String> {
    let dir: PathBuf = game_dir.join("crash-reports");
    let newest = std::fs::read_dir(&dir).ok()?.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).max_by_key(|x| x.0)?;
    if newest.0 < since {
        return None;
    }
    let text = std::fs::read_to_string(&newest.1).unwrap_or_default();
    let low = text.to_lowercase();
    Some(if low.contains("out of memory") {
        "Horizon a manqué de mémoire vidéo. Oxyde a baissé la distance de rendu de ce profil : relance, ça devrait tenir.".into()
    } else {
        let why = text.lines().find(|l| l.contains("panicked") || l.starts_with("Description")).unwrap_or("raison inconnue").trim().to_string();
        format!("Horizon a planté ({why}). Le rapport est dans le dossier crash-reports du profil.")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_render_distance() {
        let d = std::env::temp_dir().join(format!("oxyde-perf-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("options.txt"), "ao:true\nrenderDistance:64\nsimulationDistance:12\nmaxFps:120\n").unwrap();
        std::fs::write(d.join("horizon-graphics.txt"), "preset:custom\ncaveCulling:false\n").unwrap();
        let m = Machine { tier: Tier::Light, gpu: "Intel".into() };
        let c = optimize(&d, &m);
        let t = std::fs::read_to_string(d.join("options.txt")).unwrap();
        assert!(t.contains("renderDistance:10") && t.contains("simulationDistance:6") && t.contains("maxFps:60") && t.contains("ao:true"));
        assert!(std::fs::read_to_string(d.join("horizon-graphics.txt")).unwrap().contains("caveCulling:true"));
        assert!(c.len() >= 4);
        // deuxième passage : plus rien à changer
        assert!(optimize(&d, &m).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn reads_gpu_from_log() {
        let d = std::env::temp_dir().join(format!("oxyde-log-{}.log", std::process::id()));
        std::fs::write(&d, "[x] [Render thread/INFO]: Horizon renderer: Intel(R) UHD Graphics (Vulkan, IntegratedGpu)\n").unwrap();
        let m = from_log(&d).unwrap();
        assert_eq!(m.tier, Tier::Light);
        assert_eq!(m.gpu, "Intel(R) UHD Graphics");
        let _ = std::fs::remove_file(&d);
    }
}
