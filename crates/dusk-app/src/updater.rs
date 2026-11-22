//! Mises à jour automatiques avec Sparkle 2.
//!
//! `Sparkle.framework` est copié par `scripts/bundle.sh` dans `Contents/Frameworks`
//! et chargé ici à l'exécution : rien à lier à la compilation, et une app lancée
//! depuis `target/` (sans framework) fonctionne simplement sans mises à jour.
//! Tout le comportement est réglé dans l'Info.plist : vérification et installation
//! automatiques, sans rien demander à l'utilisateur.

use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyClass, AnyObject};
use objc2::msg_send;
use objc2_foundation::{NSBundle, NSString};

/// Démarre Sparkle si l'app est installée avec son framework. Le contrôleur vit
/// aussi longtemps que l'app.
pub fn start() {
    // Aperçus et captures : pas de mise à jour pendant qu'on regarde.
    if std::env::var_os("DUSK_PREVIEW").is_some() {
        return;
    }
    let main = NSBundle::mainBundle();
    let Some(dir) = main.privateFrameworksPath() else { return };
    let path = NSString::from_str(&format!("{dir}/Sparkle.framework"));
    let Some(framework) = NSBundle::bundleWithPath(&path) else { return };
    // SAFETY : framework Sparkle signé avec l'app, chargé une seule fois au lancement.
    if !unsafe { framework.load() } {
        eprintln!("[dusk] Sparkle : chargement impossible");
        return;
    }
    let Some(class) = AnyClass::get(c"SPUStandardUpdaterController") else { return };
    let none: *const AnyObject = std::ptr::null();
    let controller: Option<Retained<AnyObject>> = unsafe {
        let alloc: Allocated<AnyObject> = msg_send![class, alloc];
        msg_send![alloc, initWithStartingUpdater: true, updaterDelegate: none, userDriverDelegate: none]
    };
    if let Some(c) = controller {
        std::mem::forget(c);
    }
}
