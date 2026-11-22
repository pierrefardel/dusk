//! Détection des écrans et association CGDisplay <-> service matériel.

use crate::cg::{self, CGDirectDisplayID};
use crate::ddc::{DdcService, VCP_BRIGHTNESS};
use crate::{builtin, iokit, Error, Result};
use std::sync::Arc;

pub enum Backend {
    /// Écran externe piloté en DDC/CI.
    Ddc(Arc<DdcService>),
    /// Écran intégré ou Apple (Studio Display, XDR).
    DisplayServices,
    /// Aucun canal matériel (ex. port HDMI intégré des Mac M1/M2).
    None,
}

pub struct Display {
    pub id: CGDirectDisplayID,
    pub name: String,
    pub builtin: bool,
    pub vendor: u32,
    pub model: u32,
    pub serial: u32,
    pub backend: Backend,
}

impl Display {
    /// Identifiant stable entre redémarrages (l'ID CoreGraphics peut changer).
    pub fn key(&self) -> String {
        format!("{:04x}-{:04x}-{:08x}", self.vendor, self.model, self.serial)
    }

    pub fn backend_name(&self) -> &'static str {
        match self.backend {
            Backend::Ddc(_) => "DDC/CI",
            Backend::DisplayServices => "DisplayServices",
            Backend::None => "aucun",
        }
    }

    /// Canal matériel disponible (DDC/CI ou DisplayServices). Sans lui, Dusk règle
    /// la luminosité en assombrissant l'image (téléviseur, Sidecar, DisplayLink…).
    pub fn has_hardware(&self) -> bool {
        !matches!(self.backend, Backend::None)
    }

    /// Luminosité en pourcentage (0-100).
    pub fn brightness(&self) -> Result<f32> {
        match &self.backend {
            Backend::Ddc(s) => {
                let v = s.read(VCP_BRIGHTNESS)?;
                Ok(if v.max == 0 { 0.0 } else { v.current as f32 * 100.0 / v.max as f32 })
            }
            Backend::DisplayServices => builtin::get(self.id)
                .map(|v| v * 100.0)
                .ok_or(Error::Unsupported("DisplayServices")),
            Backend::None => Err(Error::Unsupported("aucun canal matériel")),
        }
    }

    /// Règle la luminosité en pourcentage. `max` est la valeur VCP maximale
    /// annoncée par l'écran (100 par défaut).
    pub fn set_brightness(&self, percent: f32, max: u16) -> Result<()> {
        let p = percent.clamp(0.0, 100.0);
        match &self.backend {
            Backend::Ddc(s) => s.write(VCP_BRIGHTNESS, (p / 100.0 * max as f32).round() as u16),
            Backend::DisplayServices => builtin::set(self.id, p / 100.0)
                .then_some(())
                .ok_or(Error::Unsupported("DisplayServices")),
            Backend::None => Err(Error::Unsupported("aucun canal matériel")),
        }
    }

    pub fn ddc(&self) -> Option<&DdcService> {
        match &self.backend {
            Backend::Ddc(s) => Some(s),
            _ => None,
        }
    }
}

/// Service DDC trouvé dans l'IORegistry, avec l'identité EDID du framebuffer associé.
struct AvCandidate {
    service: DdcService,
    vendor: Option<u32>,
    model: Option<u32>,
    serial: Option<u32>,
    name: Option<String>,
}

#[derive(Default, Clone)]
struct FbIdentity {
    vendor: Option<u32>,
    model: Option<u32>,
    serial: Option<u32>,
    name: Option<String>,
}

/// Sur Apple Silicon, chaque framebuffer (`AppleCLCD2` / `IOMobileFramebufferShim`)
/// est suivi dans l'arbre par son `DCPAVServiceProxy`. On les apparie dans l'ordre.
fn scan_av_services() -> Vec<AvCandidate> {
    let mut out = Vec::new();
    let mut current = FbIdentity::default();
    iokit::walk_service_plane(|entry, name| match name {
        "AppleCLCD2" | "IOMobileFramebufferShim" => {
            current = read_identity(entry);
        }
        "DCPAVServiceProxy" => {
            let external = iokit::property(entry, "Location")
                .and_then(|v| iokit::as_string(&v))
                .is_some_and(|l| l == "External");
            if external {
                if let Some(service) = DdcService::from_entry(entry) {
                    let id = std::mem::take(&mut current);
                    out.push(AvCandidate {
                        service,
                        vendor: id.vendor,
                        model: id.model,
                        serial: id.serial,
                        name: id.name,
                    });
                }
            }
        }
        _ => {}
    });
    out
}

fn read_identity(entry: iokit::IoObject) -> FbIdentity {
    let attrs = iokit::property(entry, "DisplayAttributes")
        .and_then(|v| iokit::as_dict(&v))
        .and_then(|d| iokit::dict_get(&d, "ProductAttributes"))
        .and_then(|v| iokit::as_dict(&v));
    let Some(pa) = attrs else { return FbIdentity::default() };
    let num = |k: &str| iokit::dict_get(&pa, k).and_then(|v| iokit::as_i64(&v)).map(|n| n as u32);
    FbIdentity {
        vendor: num("LegacyManufacturerID"),
        model: num("ProductID"),
        serial: num("SerialNumber"),
        name: iokit::dict_get(&pa, "ProductName").and_then(|v| iokit::as_string(&v)),
    }
}

/// Liste les écrans connectés et leur canal de contrôle matériel.
pub fn list_displays() -> Vec<Display> {
    let mut candidates: Vec<Option<AvCandidate>> = scan_av_services().into_iter().map(Some).collect();
    let ids = cg::online_displays();

    let mut displays: Vec<Display> = ids
        .iter()
        .map(|&id| unsafe {
            Display {
                id,
                name: String::new(),
                builtin: cg::CGDisplayIsBuiltin(id) != 0,
                vendor: cg::CGDisplayVendorNumber(id),
                model: cg::CGDisplayModelNumber(id),
                serial: cg::CGDisplaySerialNumber(id),
                backend: Backend::None,
            }
        })
        .collect();

    // 1. Correspondance exacte fabricant + modèle (+ numéro de série si disponible).
    for d in displays.iter_mut().filter(|d| !d.builtin) {
        let pos = candidates.iter().position(|c| {
            c.as_ref().is_some_and(|c| {
                c.vendor == Some(d.vendor)
                    && c.model == Some(d.model)
                    && (d.serial == 0 || c.serial.is_none_or(|s| s == d.serial))
            })
        });
        if let Some(c) = pos.and_then(|i| candidates[i].take()) {
            d.name = c.name.unwrap_or_default();
            d.backend = Backend::Ddc(Arc::new(c.service));
        }
    }
    // 2. Repli : appariement par ordre pour les écrans restants.
    let mut rest = candidates.into_iter().flatten();
    for d in displays.iter_mut().filter(|d| !d.builtin && matches!(d.backend, Backend::None)) {
        if builtin::can_change(d.id) {
            continue;
        }
        if let Some(c) = rest.next() {
            d.name = c.name.unwrap_or_default();
            d.backend = Backend::Ddc(Arc::new(c.service));
        }
    }
    // 3. Écran intégré et écrans Apple.
    for d in displays.iter_mut().filter(|d| matches!(d.backend, Backend::None)) {
        if builtin::can_change(d.id) {
            d.backend = Backend::DisplayServices;
        }
    }
    // Test : DUSK_SOFTWARE=<nom> fait passer cet écran par l'assombrissement de l'image.
    if let Ok(name) = std::env::var("DUSK_SOFTWARE") {
        for d in displays.iter_mut().filter(|d| d.name.eq_ignore_ascii_case(&name)) {
            d.backend = Backend::None;
        }
    }
    for d in displays.iter_mut().filter(|d| d.name.is_empty()) {
        d.name = if d.builtin { "Écran intégré".into() } else { format!("Écran {}", d.id) };
    }
    displays
}
