//! Panneau complet d'un écran : réglages proposés d'après ses capacités MCCS.
//!
//! Le cœur ne fournit que des identifiants (codes VCP, valeurs) : les libellés
//! affichés sont choisis par l'app, dans la langue de l'utilisateur.

use std::collections::BTreeMap;

pub const VCP_FACTORY_RESET: u8 = 0x04;
pub const VCP_COLOR_PRESET: u8 = 0x14;
/// Modes d'image propres aux écrans LG.
pub const VCP_LG_PICTURE_MODE: u8 = 0x15;
pub const VCP_RED_GAIN: u8 = 0x16;
pub const VCP_GREEN_GAIN: u8 = 0x18;
pub const VCP_BLUE_GAIN: u8 = 0x1A;
pub const VCP_SHARPNESS: u8 = 0x87;
pub const VCP_MUTE: u8 = 0x8D;
pub const VCP_OSD_LANGUAGE: u8 = 0xCC;
pub const VCP_POWER_MODE: u8 = 0xD6;
/// Mode d'image standard MCCS (« Display Application »).
pub const VCP_DISPLAY_MODE: u8 = 0xDC;

/// Valeurs de 0x14 : préréglages de couleur.
pub const PRESET_USER_1: u8 = 0x0B;
/// Valeurs de 0xD6 : marche, veille, arrêt.
pub const POWER_ON: u8 = 0x01;
pub const POWER_STANDBY: u8 = 0x04;
pub const POWER_OFF: u8 = 0x05;
/// Valeurs de 0x8D.
pub const MUTE_ON: u8 = 0x01;
pub const MUTE_OFF: u8 = 0x02;

/// Fabricant LG (identifiant EDID « GSM »).
pub const VENDOR_LG: u32 = 0x1E6D;

/// Réglage à valeurs listées (mode d'image, couleur, entrée, langue).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub code: u8,
    pub values: Vec<u8>,
    /// Valeur actuelle : lue sur l'écran, ou dernière valeur réglée depuis Dusk.
    pub current: Option<u8>,
}

/// Ce que l'écran annonce, au-delà de la luminosité, du contraste et du volume.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Panel {
    /// Capacités lues : sinon le panneau reste vide (mode découverte, à venir).
    pub known: bool,
    pub picture_mode: Option<Choice>,
    pub color_preset: Option<Choice>,
    /// Gains rouge, vert, bleu.
    pub rgb: bool,
    pub sharpness: bool,
    pub mute: bool,
    pub input: Option<Choice>,
    pub osd_language: Option<Choice>,
    /// Valeur à envoyer pour éteindre l'écran (veille de préférence).
    pub power_off: Option<u8>,
    pub factory_reset: bool,
}

impl Panel {
    /// Construit le panneau à partir de la liste VCP des capacités.
    pub fn from_caps(caps: &BTreeMap<u8, Vec<u8>>, vendor: u32) -> Panel {
        let choice = |code: u8| {
            caps.get(&code)
                .filter(|v| v.len() >= 2)
                .map(|v| Choice { code, values: v.clone(), current: None })
        };
        let picture_mode = choice(VCP_DISPLAY_MODE).or_else(|| {
            (vendor == VENDOR_LG).then(|| choice(VCP_LG_PICTURE_MODE)).flatten()
        });
        let power_off = caps.get(&VCP_POWER_MODE).and_then(|v| {
            [POWER_STANDBY, POWER_OFF].into_iter().find(|p| v.contains(p))
        });
        Panel {
            known: !caps.is_empty(),
            picture_mode,
            color_preset: choice(VCP_COLOR_PRESET),
            rgb: [VCP_RED_GAIN, VCP_GREEN_GAIN, VCP_BLUE_GAIN].iter().all(|c| caps.contains_key(c)),
            sharpness: caps.contains_key(&VCP_SHARPNESS),
            mute: caps.contains_key(&VCP_MUTE),
            input: choice(crate::ddc::VCP_INPUT_SOURCE),
            osd_language: choice(VCP_OSD_LANGUAGE),
            power_off,
            factory_reset: caps.contains_key(&VCP_FACTORY_RESET),
        }
    }

    /// Réglages à valeurs listées, pour lire ou restaurer leur valeur.
    pub fn choices_mut(&mut self) -> impl Iterator<Item = &mut Choice> {
        [&mut self.picture_mode, &mut self.color_preset, &mut self.input, &mut self.osd_language]
            .into_iter()
            .flatten()
    }

    pub fn choice(&self, code: u8) -> Option<&Choice> {
        [&self.picture_mode, &self.color_preset, &self.input, &self.osd_language]
            .into_iter()
            .flatten()
            .find(|c| c.code == code)
    }

    /// Le RVB n'a de sens qu'en couleur « sur mesure » (ou sans préréglages).
    pub fn rgb_active(&self) -> bool {
        self.rgb
            && match &self.color_preset {
                Some(p) => !p.values.contains(&PRESET_USER_1) || p.current == Some(PRESET_USER_1),
                None => true,
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddc::parse_vcp_list;

    const LG: &str = "(prot(monitor)type(lcd)model(WK95U)cmds(01 02 03 0C E3 F3)vcp(02 04 05 08 10 12 14(05 08 0B ) 16 18 1A 52 60(11 12 0F 10 ) AC AE B2 B6 C0 C6 C8 C9 D6(01 04) DF 62 8D F4 F5(01 02 03 04) F6(00 01 02) 4D 4E 4F 15(01 06 11 13 14 15 18 19 20 22 23 24 28 29 32 48) F7(00 01 02 03) F8(00 01) F9 EF FA(00 01) FD(00 01) FE(00 01 02) FF)mccs_ver(2.1)mswhql(1))";
    const ARZOPA: &str = "(prot(monitor)type(LCD)model(RTK)cmds(01 02 03 07 0C E3 F3)vcp(02 04 05 06 08 0B 0C 10 12 14(01 02 04 05 06 08 0B) 16 18 1A 52 60(01 03 04 0F 10 11 12) 87 AC AE B2 B6 C6 C8 CA CC(01 02 03 04 06 0A 0D) D6(01 04 05) DF FD FF)mswhql(1)asset_eep(40)mccs_ver(2.2))";

    #[test]
    fn panneau_lg() {
        let p = Panel::from_caps(&parse_vcp_list(LG), VENDOR_LG);
        assert_eq!(p.picture_mode.as_ref().map(|c| c.code), Some(VCP_LG_PICTURE_MODE));
        assert_eq!(p.color_preset.as_ref().unwrap().values, vec![0x05, 0x08, 0x0B]);
        assert!(p.rgb && p.mute && p.factory_reset && !p.sharpness);
        assert_eq!(p.power_off, Some(POWER_STANDBY));
        assert!(p.osd_language.is_none());
    }

    #[test]
    fn panneau_arzopa() {
        let p = Panel::from_caps(&parse_vcp_list(ARZOPA), 0x1EE4);
        assert!(p.picture_mode.is_none());
        assert!(p.sharpness && !p.mute);
        assert_eq!(p.osd_language.as_ref().unwrap().values.len(), 7);
        // Pas de RVB tant que la couleur n'est pas « sur mesure ».
        let mut p = p;
        p.color_preset.as_mut().unwrap().current = Some(0x05);
        assert!(!p.rgb_active());
        p.color_preset.as_mut().unwrap().current = Some(PRESET_USER_1);
        assert!(p.rgb_active());
    }

    #[test]
    fn modes_lg_reserves_aux_ecrans_lg() {
        let p = Panel::from_caps(&parse_vcp_list(LG), 0x1234);
        assert!(p.picture_mode.is_none());
    }
}
