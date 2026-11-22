//! Textes de l'interface, dans la langue du système (français par défaut).
//!
//! Chaque texte a une clé ; une langue qui ne connaît pas une clé retombe sur
//! le français. Le français tutoie l'utilisateur.

use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Fr,
    En,
}

/// Langue choisie dans Personnalisation (`None` : celle du système).
static CHOICE: std::sync::Mutex<Option<Lang>> = std::sync::Mutex::new(None);

/// « fr », « en », ou autre chose pour suivre le système.
pub fn set_language(code: &str) {
    *CHOICE.lock().unwrap() = match code {
        "fr" => Some(Lang::Fr),
        "en" => Some(Lang::En),
        _ => None,
    };
}

pub fn lang() -> Lang {
    CHOICE.lock().unwrap().unwrap_or_else(system_lang)
}

/// Langue du système (`DUSK_LANG` pour la mise au point).
pub fn system_lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| {
        if let Ok(l) = std::env::var("DUSK_LANG") {
            return if l.starts_with("en") { Lang::En } else { Lang::Fr };
        }
        let langs = objc2_foundation::NSLocale::preferredLanguages();
        let first = langs.iter().next().map(|s| s.to_string()).unwrap_or_default();
        if first.starts_with("fr") || first.is_empty() { Lang::Fr } else { Lang::En }
    })
}

/// Nom d'une langue, dans cette langue.
pub fn lang_name(l: Lang) -> &'static str {
    match l {
        Lang::Fr => "Français",
        Lang::En => "English",
    }
}

/// Texte pour une clé.
pub fn t(key: &str) -> &'static str {
    let table = match lang() {
        Lang::Fr => FR,
        Lang::En => EN,
    };
    find(table, key).or_else(|| find(FR, key)).unwrap_or("?")
}

/// Phrase d'usage associée à une clé (`clé.hint`), si elle existe.
pub fn hint(key: &str) -> Option<&'static str> {
    let k = format!("{key}.hint");
    let table = match lang() {
        Lang::Fr => FR,
        Lang::En => EN,
    };
    find(table, &k).or_else(|| find(FR, &k))
}

/// Texte avec `{x}` remplacé.
/// Valeur d'un curseur de luminosité : pourcentage, ou « Plus sombre » sous zéro.
pub fn level(v: f32) -> String {
    if v < 0.0 {
        t("dim.value").to_string()
    } else {
        format!("{v:.0} %")
    }
}

pub fn tx(key: &str, x: &str) -> String {
    t(key).replace("{x}", x)
}

fn find(table: &'static [(&'static str, &'static str)], key: &str) -> Option<&'static str> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

// --- Libellés des valeurs annoncées par l'écran -----------------------------

/// Mode d'image standard (code 0xDC).
pub fn display_mode(v: u8) -> Option<&'static str> {
    let k = match v {
        0x00 | 0x08 => "mode.standard",
        0x01 => "mode.office",
        0x02 => "mode.mixed",
        0x03 => "mode.cinema",
        0x04 => "mode.user",
        0x05 => "mode.game",
        0x06 => "mode.sport",
        0x07 => "mode.pro",
        0x09 => "mode.demo",
        0xF0 => "mode.dynamic",
        _ => return None,
    };
    Some(t(k))
}

/// Phrase qui dit à quoi sert un mode d'image standard.
pub fn display_mode_hint(v: u8) -> Option<&'static str> {
    let k = match v {
        0x00 | 0x08 => "mode.standard.hint",
        0x01 => "mode.office.hint",
        0x03 => "mode.cinema.hint",
        0x05 => "mode.game.hint",
        0x06 => "mode.sport.hint",
        _ => return None,
    };
    Some(t(k))
}

/// Préréglage de couleur (code 0x14).
pub fn color_preset(v: u8) -> String {
    let k = match v {
        0x01 => return "sRGB".into(),
        0x02 => "preset.native",
        0x03 => "preset.warmer",
        0x04 => "preset.warm",
        0x05 => "preset.neutral",
        0x06 => "preset.fresh",
        0x07 => "preset.bluish",
        0x08 => "preset.cool",
        0x09 | 0x0A => "preset.cooler",
        0x0B => "preset.custom",
        0x0C => return tx("preset.custom_n", "2"),
        0x0D => return tx("preset.custom_n", "3"),
        _ => return tx("value.n", &format!("{v}")),
    };
    t(k).into()
}

/// Entrée vidéo (code 0x60). Le numéro n'apparaît que si l'écran en a plusieurs du même type.
pub fn input(v: u8, all: &[u8]) -> String {
    let (name, group): (&str, &[u8]) = match v {
        0x01 | 0x02 => ("VGA", &[0x01, 0x02]),
        0x03 | 0x04 => ("DVI", &[0x03, 0x04]),
        0x05 | 0x06 => ("Composite", &[0x05, 0x06]),
        0x07 | 0x08 => ("S-Video", &[0x07, 0x08]),
        0x09..=0x0B => ("TV", &[0x09, 0x0A, 0x0B]),
        0x0C..=0x0E => ("Component", &[0x0C, 0x0D, 0x0E]),
        0x0F | 0x10 => ("DisplayPort", &[0x0F, 0x10]),
        0x11 | 0x12 => ("HDMI", &[0x11, 0x12]),
        0x1B => ("USB-C", &[0x1B]),
        _ => return tx("input.n", &format!("{v}")),
    };
    let siblings: Vec<u8> = group.iter().copied().filter(|g| all.contains(g)).collect();
    if siblings.len() > 1 {
        let n = siblings.iter().position(|&g| g == v).unwrap_or(0) + 1;
        format!("{name} {n}")
    } else {
        name.to_string()
    }
}

/// Langue des menus de l'écran (code 0xCC), dans sa propre langue.
pub fn osd_language(v: u8) -> String {
    let s = match v {
        0x01 => "中文（繁體）",
        0x02 => "English",
        0x03 => "Français",
        0x04 => "Deutsch",
        0x05 => "Italiano",
        0x06 => "日本語",
        0x07 => "한국어",
        0x08 => "Português",
        0x09 => "Русский",
        0x0A => "Español",
        0x0B => "Svenska",
        0x0C => "Türkçe",
        0x0D => "中文（简体）",
        0x0E => "Português (Brasil)",
        0x0F => "العربية",
        0x10 => "Български",
        0x11 => "Hrvatski",
        0x12 => "Čeština",
        0x13 => "Dansk",
        0x14 => "Nederlands",
        0x15 => "Eesti",
        0x16 => "Suomi",
        0x17 => "Ελληνικά",
        0x18 => "עברית",
        0x19 => "हिन्दी",
        0x1A => "Magyar",
        0x1B => "Latviešu",
        0x1C => "Lietuvių",
        0x1D => "Norsk",
        0x1E => "Polski",
        0x1F => "Română",
        0x20 => "Српски",
        0x21 => "Slovenčina",
        0x22 => "Slovenščina",
        0x23 => "ไทย",
        0x24 => "Українська",
        0x25 => "Tiếng Việt",
        _ => return tx("value.n", &format!("{v}")),
    };
    s.into()
}

const FR: &[(&str, &str)] = &[
    ("mode.custom", "Personnalisé"),
    ("mode.custom.hint", "Les réglages que tu as faits sur l'écran."),
    ("mode.photo", "Photo"),
    ("mode.photo.hint", "Couleurs fidèles pour retoucher ou regarder des photos."),
    ("mode.cinema.hint", "Couleurs fidèles aux films."),
    ("mode.reader", "Lecture"),
    ("mode.reader.hint", "Moins de lumière bleue, pour lire longtemps."),
    ("mode.fps1", "Jeu d'action 1"),
    ("mode.fps1.hint", "Les zones sombres ressortent, pour repérer plus vite."),
    ("mode.fps2", "Jeu d'action 2"),
    ("mode.fps2.hint", "Variante plus marquée pour les jeux d'action."),
    ("mode.rts", "Jeu de stratégie"),
    ("mode.rts.hint", "Image nette et contrastée pour les parties longues."),
    ("mode.custom_game", "Jeu personnalisé"),
    ("mode.custom_game.hint", "Tes réglages de jeu faits sur l'écran."),
    ("mode.dark1", "Pièce sombre 1"),
    ("mode.dark1.hint", "Image adoucie pour une pièce peu éclairée."),
    ("mode.dark2", "Pièce sombre 2"),
    ("mode.dark2.hint", "Encore plus doux, pour le noir complet."),
    ("mode.color_weak", "Couleurs adaptées"),
    ("mode.color_weak.hint", "Aide à distinguer les couleurs proches."),
    ("nav.displays", "Écrans"),
    ("nav.keys", "Raccourcis"),
    ("nav.auto", "Automatique"),
    ("auto.subtitle", "Dusk ajuste tes écrans tout seul, au bon moment. Dès que tu bouges un curseur, ta main passe en premier."),
    ("solar.title", "Suivre le soleil"),
    ("solar.desc", "Plus lumineux le jour, plus doux le soir. Dusk se base sur l'heure et le lieu de ton Mac."),
    ("solar.sunrise", "Lever {x}"),
    ("solar.sunset", "Coucher {x}"),
    ("solar.midnight", "Minuit"),
    ("solar.now", "Maintenant · {x}"),
    ("solar.day_short", "Jour {x}"),
    ("solar.night_short", "Nuit {x}"),
    ("solar.learned", "appris de tes réglages"),
    ("solar.adjust", "Ajuster la courbe ›"),
    ("solar.back", "‹ Automatique"),
    ("solar.detail", "Tu n'as rien à régler : Dusk retient la luminosité que tu choisis le jour et le soir, et passe de l'une à l'autre en douceur."),
    ("solar.day", "En journée"),
    ("solar.night", "Le soir et la nuit"),
    ("solar.learn_hint", "Ces valeurs bougent toutes seules quand tu changes la luminosité depuis le menu ou le clavier : Dusk s'en souvient pour ce moment de la journée."),
    ("solar.speed", "Passage jour / nuit"),
    ("solar.speed.hint", "Sur {x} minutes autour du lever et du coucher."),
    ("solar.speed.fast", "Rapide"),
    ("solar.speed.soft", "Douce"),
    ("solar.speed.slow", "Très douce"),
    ("solar.screens", "Écrans qui suivent le soleil"),
    ("solar.place", "Heures du soleil calculées pour {x} d'après le fuseau horaire de ton Mac, sans demander ta position."),
    ("solar.place_unknown", "Heures du soleil estimées d'après le fuseau horaire de ton Mac, sans demander ta position."),
    ("solar.polar", "Aujourd'hui, le soleil ne se lève pas ou ne se couche pas ici : Dusk garde ta luminosité."),
    ("pop.auto.title", "Automatique"),
    ("pop.auto.desc", "La luminosité s'adapte aux heures de la journée."),
    ("room.title", "Suivre la lumière de la pièce"),
    ("room.desc", "Ton Mac sent la lumière autour de toi : tes écrans s'éclaircissent ou s'adoucissent avec elle."),
    ("room.dark", "Pièce sombre"),
    ("room.bright", "Plein jour"),
    ("room.now", "Maintenant · {x}"),
    ("room.state.dark", "pièce sombre"),
    ("room.state.dim", "pièce tamisée"),
    ("room.state.bright", "pièce claire"),
    ("room.screens", "Écrans · {x}"),
    ("room.hint", "Tant que cette option est active, elle passe avant le soleil. Ce que tu règles à la main est retenu pour ce niveau de lumière."),
    ("solar.paused_room", "En pause tant que la lumière de la pièce est suivie."),
    ("pop.auto.room", "La luminosité s'adapte à la lumière de la pièce."),
    ("nav.personal", "Personnalisation"),
    ("nav.advanced", "Avancé"),
    ("window.title", "Réglages Dusk"),
    ("keys.subtitle", "Touches luminosité du clavier, actives même quand Dusk n'est pas au premier plan."),
    ("keys.ok", "Touches luminosité interceptées"),
    ("keys.need", "Autorise Dusk pour utiliser les touches"),
    ("keys.open", "Ouvrir les réglages"),
    ("keys.under", "Écran sous le curseur"),
    ("keys.all", "Tous les écrans"),
    ("keys.fine", "Pas fin de 1 %"),
    ("keys.fallback", "Secours, sans autorisation"),
    ("keys.step", "Pas par appui"),
    ("step.16", "6,25 % (16 crans)"),
    ("step.32", "3,125 % (32 crans)"),
    ("adv.subtitle", "Démarrage, écrans et configuration."),
    ("adv.login", "Lancer Dusk à l'ouverture de session"),
    ("adv.reapply", "Réappliquer la dernière luminosité au réveil et au rebranchement"),
    ("adv.rescan", "Redétecter les écrans"),
    ("adv.config", "Ouvrir le dossier de configuration"),
    ("pop.hello", "Bonjour"),
    ("pop.evening", "Bonsoir"),
    ("pop.all", "Tous les écrans"),
    ("dim.value", "Plus sombre"),
    ("dim.hint", "Plus sombre que ce que l'écran sait faire : Dusk assombrit l'image."),
    ("pop.displays", "ÉCRANS"),
    ("pop.settings", "Réglages"),
    ("pop.quit", "Quitter Dusk"),
    ("hud.all", "Tous les écrans · {x}"),
    ("pers.subtitle", "Les couleurs et la langue de Dusk, à ton goût."),
    ("pers.theme", "Thème"),
    ("theme.minuit", "Minuit magique"),
    ("theme.minuit.sub", "Indigo et violet, étincelle dorée"),
    ("theme.ambre", "Ambre nuit"),
    ("theme.ambre.sub", "Graphite et ambre chaud"),
    ("theme.brume", "Brume"),
    ("theme.brume.sub", "Sobre, proche de macOS"),
    ("pers.appearance", "Apparence"),
    ("app.dark", "Sombre"),
    ("app.light", "Clair"),
    ("app.system", "Système"),
    ("pers.app.system_hint", "Suit le réglage de ton Mac."),
    ("pers.app.fixed_hint", "Dusk garde cette apparence, quel que soit le réglage du Mac."),
    ("pers.language", "Langue"),
    ("pers.language.hint", "Par défaut, celle de ton Mac."),
    ("lang.auto", "Automatique ({x})"),
    ("displays.title", "Écrans"),
    ("displays.subtitle", "Tout ce que ton écran sait faire, sans chercher ses boutons."),
    ("card.image", "Image"),
    ("brightness", "Luminosité"),
    ("contrast", "Contraste"),
    ("card.mode", "Mode d'image"),
    ("card.color", "Couleur"),
    ("color.custom_hint", "Choisis « Sur mesure » pour doser le rouge, le vert et le bleu."),
    ("red", "Rouge"),
    ("green", "Vert"),
    ("blue", "Bleu"),
    ("card.sound", "Son"),
    ("volume", "Volume"),
    ("mute", "Couper le son"),
    ("unmute", "Remettre le son"),
    ("muted", "Son coupé"),
    ("card.input", "Entrée"),
    ("this_mac", "ce Mac"),
    ("input.here", "Ton Mac est branché ici."),
    ("input.unknown", "L'écran affichera l'appareil branché sur l'entrée choisie."),
    ("input.confirm.title", "Passer sur {x} ?"),
    ("input.confirm.body", "L'écran va afficher l'appareil branché sur {x}. Pour revenir, utilise les boutons de l'écran."),
    ("input.confirm.ok", "Changer d'entrée"),
    ("input.n", "Entrée {x}"),
    ("cancel", "Annuler"),
    ("undo", "Annuler mes changements"),
    ("more", "Plus de réglages"),
    ("less", "Moins de réglages"),
    ("sharpness", "Netteté"),
    ("osd_language", "Langue des menus de l'écran"),
    ("sleep", "Éteindre l'écran"),
    ("sleep.hint", "L'écran s'éteint, tes fenêtres passent sur les autres écrans."),
    ("sleep.btn", "Éteindre"),
    ("wake.btn", "Rallumer"),
    ("sleeping.title", "{x} est éteint"),
    ("sleeping.hint", "Le Mac ne l'utilise plus. Si Dusk quitte, il se rallume tout seul."),
    ("reset", "Réglages d'usine"),
    ("reset.hint", "Remet cet écran comme au premier jour."),
    ("reset.btn", "Réinitialiser…"),
    ("reset.confirm.title", "Réinitialiser {x} ?"),
    ("reset.confirm.body", "Luminosité, couleurs et modes reviennent aux valeurs d'usine. Ça ne touche pas aux autres écrans."),
    ("reset.confirm.ok", "Réinitialiser"),
    ("footer", "Seuls les réglages que cet écran accepte sont affichés."),
    ("soft.title", "Dusk s'en occupe à sa place"),
    ("soft.desc", "Cet écran ne se règle pas depuis le Mac. Dusk assombrit l'image lui-même : la luminosité marche comme pour tes autres écrans, au clavier et en automatique. Le reste se règle avec ses boutons ou sa télécommande."),
    ("note.write_only", "Cet écran n'indique pas ses réglages actuels : Dusk affiche les derniers réglés depuis Dusk."),
    ("extras.check", "Afficher contraste et volume"),
    ("extras.hint", "À activer si l'écran possède ces réglages : il ne permet pas à Dusk de le vérifier."),
    ("value.n", "Réglage {x}"),
    ("mode.n", "Mode {x}"),
    ("mode.standard", "Standard"),
    ("mode.office", "Bureautique"),
    ("mode.mixed", "Polyvalent"),
    ("mode.cinema", "Cinéma"),
    ("mode.user", "Personnalisé"),
    ("mode.game", "Jeu"),
    ("mode.sport", "Sport"),
    ("mode.pro", "Professionnel"),
    ("mode.demo", "Démo"),
    ("mode.dynamic", "Contraste dynamique"),
    ("mode.standard.hint", "Équilibré pour tout faire."),
    ("mode.office.hint", "Confortable pour les textes et les tableaux."),
        ("mode.game.hint", "Image réactive, les zones sombres ressortent."),
    ("mode.sport.hint", "Couleurs vives et mouvements nets."),
    ("preset.native", "Native"),
    ("preset.warmer", "Très chaude"),
    ("preset.warm", "Chaude"),
    ("preset.neutral", "Neutre"),
    ("preset.fresh", "Fraîche"),
    ("preset.bluish", "Bleutée"),
    ("preset.cool", "Froide"),
    ("preset.cooler", "Très froide"),
    ("preset.custom", "Sur mesure"),
    ("preset.custom_n", "Sur mesure {x}"),
    ("welcome.title", "Bienvenue dans Dusk"),
    ("welcome.lead", "Dusk règle la vraie luminosité de tes écrans, depuis la barre des menus."),
    ("welcome.displays", "Tes écrans sont prêts"),
    ("welcome.keys.title", "Tes touches de luminosité"),
    ("welcome.keys.text", "Pour que F1 et F2 règlent l'écran sous ta souris, macOS doit autoriser Dusk, une seule fois. Dusk redémarre tout seul ensuite."),
    ("welcome.login", "Ouvrir Dusk à chaque démarrage du Mac"),
    ("welcome.later", "Plus tard"),
    ("welcome.allow", "Autoriser les touches"),
    ("welcome.start", "C'est parti"),
    ("wait.title", "Une dernière étape"),
    ("wait.lead", "macOS vient d'ouvrir ses réglages. Active Dusk dans la liste."),
    ("wait.caption", "DANS LES RÉGLAGES DE MACOS"),
    ("wait.hint", "Dès que c'est fait, Dusk redémarre tout seul. Rien d'autre à faire."),
    ("wait.reopen", "Rouvrir les réglages de macOS"),
    ("ready.title", "C'est prêt"),
    ("ready.lead", "Dusk vit ici, dans la barre des menus. Clique sur son icône pour régler tes écrans."),
    ("ready.under", "L'écran sous ta souris"),
    ("ready.done", "Terminer"),
];

const EN: &[(&str, &str)] = &[
    ("mode.custom", "Custom"),
    ("mode.custom.hint", "The settings you made on the display."),
    ("mode.photo", "Photo"),
    ("mode.photo.hint", "Accurate colours for editing or viewing photos."),
    ("mode.cinema.hint", "Colours true to films."),
    ("mode.reader", "Reader"),
    ("mode.reader.hint", "Less blue light for long reading."),
    ("mode.fps1", "Action game 1"),
    ("mode.fps1.hint", "Dark areas stand out so you spot things faster."),
    ("mode.fps2", "Action game 2"),
    ("mode.fps2.hint", "A stronger variant for action games."),
    ("mode.rts", "Strategy game"),
    ("mode.rts.hint", "Crisp, contrasted picture for long sessions."),
    ("mode.custom_game", "Custom game"),
    ("mode.custom_game.hint", "Your game settings made on the display."),
    ("mode.dark1", "Dark room 1"),
    ("mode.dark1.hint", "A softer picture for a dim room."),
    ("mode.dark2", "Dark room 2"),
    ("mode.dark2.hint", "Even softer, for complete darkness."),
    ("mode.color_weak", "Colour assist"),
    ("mode.color_weak.hint", "Helps tell similar colours apart."),
    ("nav.displays", "Displays"),
    ("nav.keys", "Shortcuts"),
    ("nav.auto", "Automatic"),
    ("auto.subtitle", "Dusk adjusts your displays on its own, at the right time. Whenever you move a slider, your choice comes first."),
    ("solar.title", "Follow the sun"),
    ("solar.desc", "Brighter by day, softer in the evening. Dusk uses your Mac's time and place."),
    ("solar.sunrise", "Sunrise {x}"),
    ("solar.sunset", "Sunset {x}"),
    ("solar.midnight", "Midnight"),
    ("solar.now", "Now · {x}"),
    ("solar.day_short", "Day {x}"),
    ("solar.night_short", "Night {x}"),
    ("solar.learned", "learned from your settings"),
    ("solar.adjust", "Adjust the curve ›"),
    ("solar.back", "‹ Automatic"),
    ("solar.detail", "Nothing to set up: Dusk remembers the brightness you choose by day and in the evening, and moves gently from one to the other."),
    ("solar.day", "During the day"),
    ("solar.night", "Evening and night"),
    ("solar.learn_hint", "These values update on their own when you change brightness from the menu or the keyboard: Dusk remembers it for that time of day."),
    ("solar.speed", "Day / night change"),
    ("solar.speed.hint", "Over {x} minutes around sunrise and sunset."),
    ("solar.speed.fast", "Quick"),
    ("solar.speed.soft", "Gentle"),
    ("solar.speed.slow", "Very gentle"),
    ("solar.screens", "Displays that follow the sun"),
    ("solar.place", "Sun times worked out for {x} from your Mac's time zone, without asking for your location."),
    ("solar.place_unknown", "Sun times estimated from your Mac's time zone, without asking for your location."),
    ("solar.polar", "Today the sun doesn't rise or set here: Dusk keeps your brightness."),
    ("pop.auto.title", "Automatic"),
    ("pop.auto.desc", "Brightness adapts to the time of day."),
    ("room.title", "Follow the room light"),
    ("room.desc", "Your Mac senses the light around you: your displays brighten or soften with it."),
    ("room.dark", "Dark room"),
    ("room.bright", "Daylight"),
    ("room.now", "Now · {x}"),
    ("room.state.dark", "dark room"),
    ("room.state.dim", "dim room"),
    ("room.state.bright", "bright room"),
    ("room.screens", "Displays · {x}"),
    ("room.hint", "While this is on, it comes before the sun. What you set by hand is remembered for that light level."),
    ("solar.paused_room", "Paused while the room light is followed."),
    ("pop.auto.room", "Brightness adapts to the room light."),
    ("nav.personal", "Personalisation"),
    ("nav.advanced", "Advanced"),
    ("window.title", "Dusk Settings"),
    ("keys.subtitle", "Keyboard brightness keys, active even when Dusk isn't in front."),
    ("keys.ok", "Brightness keys captured"),
    ("keys.need", "Allow Dusk to use the keys"),
    ("keys.open", "Open settings"),
    ("keys.under", "Display under the pointer"),
    ("keys.all", "All displays"),
    ("keys.fine", "Fine 1 % step"),
    ("keys.fallback", "Fallback, no permission needed"),
    ("keys.step", "Step per press"),
    ("step.16", "6.25 % (16 steps)"),
    ("step.32", "3.125 % (32 steps)"),
    ("adv.subtitle", "Startup, displays and configuration."),
    ("adv.login", "Open Dusk at login"),
    ("adv.reapply", "Reapply the last brightness on wake and reconnection"),
    ("adv.rescan", "Detect displays again"),
    ("adv.config", "Open configuration folder"),
    ("pop.hello", "Hello"),
    ("pop.evening", "Good evening"),
    ("pop.all", "All displays"),
    ("dim.value", "Extra dim"),
    ("dim.hint", "Darker than the display can go: Dusk dims the picture."),
    ("pop.displays", "DISPLAYS"),
    ("pop.settings", "Settings"),
    ("pop.quit", "Quit Dusk"),
    ("hud.all", "All displays · {x}"),
    ("pers.subtitle", "Dusk's colours and language, your way."),
    ("pers.theme", "Theme"),
    ("theme.minuit", "Magic midnight"),
    ("theme.minuit.sub", "Indigo and violet, golden spark"),
    ("theme.ambre", "Amber night"),
    ("theme.ambre.sub", "Graphite and warm amber"),
    ("theme.brume", "Mist"),
    ("theme.brume.sub", "Understated, close to macOS"),
    ("pers.appearance", "Appearance"),
    ("app.dark", "Dark"),
    ("app.light", "Light"),
    ("app.system", "System"),
    ("pers.app.system_hint", "Follows your Mac's setting."),
    ("pers.app.fixed_hint", "Dusk keeps this appearance whatever the Mac uses."),
    ("pers.language", "Language"),
    ("pers.language.hint", "By default, your Mac's."),
    ("lang.auto", "Automatic ({x})"),
    ("displays.title", "Displays"),
    ("displays.subtitle", "Everything your display can do, no buttons to hunt for."),
    ("card.image", "Picture"),
    ("brightness", "Brightness"),
    ("contrast", "Contrast"),
    ("card.mode", "Picture mode"),
    ("card.color", "Colour"),
    ("color.custom_hint", "Pick “Custom” to fine-tune red, green and blue."),
    ("red", "Red"),
    ("green", "Green"),
    ("blue", "Blue"),
    ("card.sound", "Sound"),
    ("volume", "Volume"),
    ("mute", "Mute"),
    ("unmute", "Unmute"),
    ("muted", "Muted"),
    ("card.input", "Input"),
    ("this_mac", "this Mac"),
    ("input.here", "Your Mac is plugged in here."),
    ("input.unknown", "The display will show the device plugged into the chosen input."),
    ("input.confirm.title", "Switch to {x}?"),
    ("input.confirm.body", "The display will show the device plugged into {x}. To come back, use the display's buttons."),
    ("input.confirm.ok", "Switch input"),
    ("input.n", "Input {x}"),
    ("cancel", "Cancel"),
    ("undo", "Undo my changes"),
    ("more", "More settings"),
    ("less", "Fewer settings"),
    ("sharpness", "Sharpness"),
    ("osd_language", "Display menu language"),
    ("sleep", "Turn display off"),
    ("sleep.hint", "The display goes dark, your windows move to the other displays."),
    ("sleep.btn", "Turn off"),
    ("wake.btn", "Turn on"),
    ("sleeping.title", "{x} is off"),
    ("sleeping.hint", "The Mac no longer uses it. If Dusk quits, it turns back on by itself."),
    ("reset", "Factory settings"),
    ("reset.hint", "Puts this display back as it came out of the box."),
    ("reset.btn", "Reset…"),
    ("reset.confirm.title", "Reset {x}?"),
    ("reset.confirm.body", "Brightness, colours and modes go back to factory values. Other displays aren't touched."),
    ("reset.confirm.ok", "Reset"),
    ("footer", "Only the settings this display accepts are shown."),
    ("soft.title", "Dusk handles it instead"),
    ("soft.desc", "This display can't be adjusted from the Mac. Dusk dims the picture itself: brightness works like on your other displays, from the keyboard and automatically. Everything else is set with its buttons or remote."),
    ("note.write_only", "This display doesn't report its current settings: Dusk shows the last ones set from Dusk."),
    ("extras.check", "Show contrast and volume"),
    ("extras.hint", "Turn on if the display has these settings: it doesn't let Dusk check."),
    ("value.n", "Setting {x}"),
    ("mode.n", "Mode {x}"),
    ("mode.standard", "Standard"),
    ("mode.office", "Office"),
    ("mode.mixed", "Mixed"),
    ("mode.cinema", "Cinema"),
    ("mode.user", "Custom"),
    ("mode.game", "Game"),
    ("mode.sport", "Sport"),
    ("mode.pro", "Professional"),
    ("mode.demo", "Demo"),
    ("mode.dynamic", "Dynamic contrast"),
    ("mode.standard.hint", "Balanced for everything."),
    ("mode.office.hint", "Comfortable for text and spreadsheets."),
        ("mode.game.hint", "Responsive picture, dark areas stand out."),
    ("mode.sport.hint", "Vivid colours and crisp motion."),
    ("preset.native", "Native"),
    ("preset.warmer", "Very warm"),
    ("preset.warm", "Warm"),
    ("preset.neutral", "Neutral"),
    ("preset.fresh", "Fresh"),
    ("preset.bluish", "Bluish"),
    ("preset.cool", "Cool"),
    ("preset.cooler", "Very cool"),
    ("preset.custom", "Custom"),
    ("preset.custom_n", "Custom {x}"),
    ("welcome.title", "Welcome to Dusk"),
    ("welcome.lead", "Dusk sets the real brightness of your displays, right from the menu bar."),
    ("welcome.displays", "Your displays are ready"),
    ("welcome.keys.title", "Your brightness keys"),
    ("welcome.keys.text", "For F1 and F2 to adjust the display under your pointer, macOS needs to allow Dusk, just once. Dusk restarts on its own afterwards."),
    ("welcome.login", "Open Dusk every time your Mac starts"),
    ("welcome.later", "Later"),
    ("welcome.allow", "Allow the keys"),
    ("welcome.start", "Let's go"),
    ("wait.title", "One last step"),
    ("wait.lead", "macOS just opened its settings. Turn Dusk on in the list."),
    ("wait.caption", "IN MACOS SETTINGS"),
    ("wait.hint", "As soon as it's done, Dusk restarts on its own. Nothing else to do."),
    ("wait.reopen", "Reopen macOS settings"),
    ("ready.title", "All set"),
    ("ready.lead", "Dusk lives here, in the menu bar. Click its icon to adjust your displays."),
    ("ready.under", "The display under your pointer"),
    ("ready.done", "Done"),
];
