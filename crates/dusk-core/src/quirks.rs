//! Exceptions par fabricant : ce que la norme MCCS ne dit pas, ou que l'écran annonce mal.
//!
//! Chaque écran testé enrichit ce fichier.

use crate::panel::VENDOR_LG;

/// Entrée vidéo pilotée par un code propre au fabricant.
pub struct InputQuirk {
    pub code: u8,
    /// Message envoyé depuis l'adresse source alternative (0x50).
    pub alt_address: bool,
    /// Valeur standard (0x60) -> valeur du fabricant.
    pub map: &'static [(u8, u16)],
}

#[derive(Default)]
pub struct Quirks {
    pub input: Option<&'static InputQuirk>,
    /// Entrée annoncée mais sans effet connu : on ne la propose pas.
    pub hide_input: bool,
    /// Noms d'entrées qui diffèrent de la norme (valeur standard -> nom).
    pub input_labels: &'static [(u8, &'static str)],
    /// Modes d'image propres au fabricant : (valeur, clé de texte), dans l'ordre d'affichage.
    /// Les valeurs absentes sont inconnues et ne sont pas proposées.
    pub modes: &'static [(u8, &'static str)],
}

/// Modes LG (code 0x15), relevés par la communauté ddccontrol sur le 27UD58 et
/// présents à l'identique dans les capacités du 34WK95U.
static LG_MODES: &[(u8, &str)] = &[
    (0x11, "mode.custom"),
    (0x32, "mode.photo"),
    (0x48, "mode.cinema"),
    (0x01, "mode.reader"),
    (0x28, "mode.fps1"),
    (0x29, "mode.fps2"),
    (0x13, "mode.rts"),
    (0x14, "mode.custom_game"),
    (0x09, "mode.dark1"),
    (0x10, "mode.dark2"),
    (0x06, "mode.color_weak"),
];

/// LG : l'entrée standard (0x60) est ignorée. Codes 0xF4 depuis l'adresse 0x50 connus
/// de la communauté, mais sans effet sur le 34WK95U relié en HDMI (2026-11-21) : entrée masquée.
#[allow(dead_code)]
static LG_INPUT: InputQuirk = InputQuirk {
    code: 0xF4,
    alt_address: true,
    map: &[(0x11, 0x90), (0x12, 0x91), (0x0F, 0xD0), (0x10, 0xD1)],
};

pub fn for_vendor(vendor: u32) -> Quirks {
    match vendor {
        VENDOR_LG => Quirks { input: None, hide_input: true, input_labels: &[(0x10, "USB-C")], modes: LG_MODES },
        _ => Quirks::default(),
    }
}

/// Nom d'entrée propre au fabricant, s'il diffère de la norme.
pub fn input_label(vendor: u32, value: u8) -> Option<&'static str> {
    for_vendor(vendor).input_labels.iter().find(|(v, _)| *v == value).map(|(_, n)| *n)
}
