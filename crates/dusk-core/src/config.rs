//! Configuration persistée dans `~/Library/Application Support/Dusk/config.json`.

use crate::sync::Curve;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub master: f32,
    /// Courbes de synchro par écran (clé : `Display::key`).
    pub curves: HashMap<String, Curve>,
    /// Pas des raccourcis clavier, en points de pourcentage.
    pub step: f32,
    /// Dernière luminosité envoyée par écran (sert aux écrans en écriture seule).
    pub last: HashMap<String, f32>,
    /// Réappliquer la dernière luminosité au réveil et au rebranchement.
    pub reapply_on_wake: bool,
    /// Contraste / volume activés à la main pour les écrans qui ne répondent pas.
    pub extras: HashMap<String, bool>,
    /// Dernières valeurs de contraste / volume par écran (clé : code VCP).
    pub last_controls: HashMap<String, HashMap<u8, f32>>,
    /// Chaîne de capacités MCCS par écran (lue une fois, ~1 s sur le bus).
    pub caps: HashMap<String, String>,
    /// Dernières valeurs listées réglées depuis Dusk (mode d'image, couleur…), par code VCP.
    pub last_choices: HashMap<String, HashMap<u8, u8>>,
    /// Réglages refusés par l'écran (vérifiés par relecture), masqués : codes VCP par écran.
    pub refused: HashMap<String, Vec<u8>>,
    /// Thème de l'interface : « minuit », « ambre » ou « brume ».
    pub theme: String,
    /// Apparence : « dark », « light » ou « system ».
    pub appearance: String,
    /// Langue : « auto », « fr » ou « en ».
    pub language: String,
    /// Suivre le soleil.
    pub solar: Solar,
    /// Suivre la lumière de la pièce (si le Mac a un capteur) : passe avant le soleil.
    pub room: bool,
}

/// Réglages de « Suivre le soleil ».
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Solar {
    pub enabled: bool,
    /// Durée du passage jour / nuit, en minutes.
    pub transition: f32,
    /// Luminosité (jour, nuit) par écran ; clé vide : curseur commun des écrans liés.
    pub levels: HashMap<String, (f32, f32)>,
    /// Écrans qui ne suivent pas le soleil.
    pub excluded: Vec<String>,
    /// Pause demandée depuis le popover, jusqu'à cet instant (secondes Unix).
    pub paused_until: i64,
    /// Écrans dont l'utilisateur a déjà réglé la luminosité (valeurs vraiment apprises).
    pub taught: Vec<String>,
}

impl Solar {
    /// Retient que l'utilisateur a réglé lui-même cet écran (clé vide : curseur commun).
    pub fn mark_taught(&mut self, key: &str) {
        if !self.taught.iter().any(|k| k == key) {
            self.taught.push(key.to_string());
        }
    }
}

impl Default for Solar {
    fn default() -> Self {
        Solar { enabled: true, transition: 40.0, levels: HashMap::new(), excluded: Vec::new(), paused_until: 0, taught: Vec::new() }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            master: 50.0,
            curves: HashMap::new(),
            step: 6.25,
            last: HashMap::new(),
            reapply_on_wake: true,
            extras: HashMap::new(),
            last_controls: HashMap::new(),
            caps: HashMap::new(),
            last_choices: HashMap::new(),
            refused: HashMap::new(),
            theme: "minuit".into(),
            appearance: "dark".into(),
            language: "auto".into(),
            solar: Solar::default(),
            room: true,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        let home = std::env::var_os("HOME").unwrap_or_default();
        PathBuf::from(home).join("Library/Application Support/Dusk/config.json")
    }

    pub fn load() -> Self {
        // L'app s'appelait Lumos : ses réglages sont repris une fois, sans rien demander.
        let path = Self::path();
        if !path.exists() {
            if let (Some(dir), Some(home)) = (path.parent(), std::env::var_os("HOME")) {
                let old = PathBuf::from(home).join("Library/Application Support/Lumos");
                if old.join("config.json").exists() && !dir.exists() {
                    let _ = std::fs::rename(&old, dir);
                }
            }
        }
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> crate::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).expect("config sérialisable"))?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}
