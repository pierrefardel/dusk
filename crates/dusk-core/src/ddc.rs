//! Protocole DDC/CI au-dessus de `IOAVService` (Apple Silicon).
//!
//! Trame d'écriture (hôte -> écran), adresse I2C 0x37, sous-adresse 0x51 :
//! `[0x80 | len, opcode, payload..., checksum]`.

use crate::iokit::{self, IOAVServiceRef, IoObject};
use crate::{Error, Result};
use std::sync::Mutex;
use std::thread::sleep;
use std::time::Duration;

pub const VCP_BRIGHTNESS: u8 = 0x10;
pub const VCP_CONTRAST: u8 = 0x12;
pub const VCP_INPUT_SOURCE: u8 = 0x60;
pub const VCP_VOLUME: u8 = 0x62;

const I2C_ADDR: u32 = 0x37;
const DATA_ADDR: u32 = 0x51;
/// Adresse source « alternative » de certains fabricants (LG : entrée via 0xF4).
const ALT_ADDR: u32 = 0x50;
const WRITE_CYCLES: usize = 2;
const WRITE_DELAY: Duration = Duration::from_millis(10);
const READ_DELAY: Duration = Duration::from_millis(50);
const READ_RETRIES: usize = 4;
/// Écart minimal entre deux messages imposé par la norme DDC/CI.
const MESSAGE_GAP: Duration = Duration::from_millis(50);

pub(crate) fn debug() -> bool {
    std::env::var_os("DUSK_DEBUG").is_some()
}

/// Valeur VCP lue sur l'écran.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VcpValue {
    pub current: u16,
    pub max: u16,
}

/// Canal DDC vers un écran externe. Les accès sont sérialisés : le bus I2C
/// ne supporte pas les transactions concurrentes.
pub struct DdcService {
    service: IOAVServiceRef,
    lock: Mutex<()>,
}

// SAFETY : IOAVServiceRef est un objet CF immuable ; les accès I2C sont protégés par `lock`.
unsafe impl Send for DdcService {}
unsafe impl Sync for DdcService {}

impl Drop for DdcService {
    fn drop(&mut self) {
        unsafe { iokit::CFRelease(self.service as *const _) }
    }
}

fn checksum(init: u8, data: &[u8]) -> u8 {
    data.iter().fold(init, |acc, b| acc ^ b)
}

impl DdcService {
    pub(crate) fn from_entry(entry: IoObject) -> Option<Self> {
        let api = iokit::av_api()?;
        let service = unsafe { (api.create_with_service)(std::ptr::null(), entry) };
        (!service.is_null()).then(|| DdcService { service, lock: Mutex::new(()) })
    }

    fn write_raw(&self, packet: &mut [u8]) -> Result<()> {
        self.write_to(DATA_ADDR, packet)
    }

    fn write_to(&self, addr: u32, packet: &mut [u8]) -> Result<()> {
        let api = iokit::av_api().ok_or(Error::Unsupported("IOAVService indisponible"))?;
        let mut ok = false;
        for _ in 0..WRITE_CYCLES {
            sleep(WRITE_DELAY);
            let r = unsafe {
                (api.write_i2c)(self.service, I2C_ADDR, addr, packet.as_mut_ptr(), packet.len() as u32)
            };
            ok |= r == 0;
        }
        ok.then_some(()).ok_or_else(|| Error::Ddc("écriture I2C refusée".into()))
    }

    /// Écrit une valeur VCP (opcode 0x03 « Set VCP Feature »).
    pub fn write(&self, code: u8, value: u16) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut p = [0x84, 0x03, code, (value >> 8) as u8, value as u8, 0];
        p[5] = checksum(0x6E ^ 0x51, &p[..5]);
        let r = self.write_raw(&mut p);
        if debug() {
            eprintln!("[ddc] écriture VCP {code:#04x} = {value} ({value:#06x}) : {}", if r.is_ok() { "acceptée par le bus" } else { "refusée" });
        }
        // Laisse l'écran traiter avant le message suivant (verrou toujours tenu).
        sleep(MESSAGE_GAP);
        r
    }

    /// Écrit une valeur VCP depuis l'adresse source alternative (0x50).
    pub fn write_alt(&self, code: u8, value: u16) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        let mut p = [0x84, 0x03, code, (value >> 8) as u8, value as u8, 0];
        p[5] = checksum(0x6E ^ ALT_ADDR as u8, &p[..5]);
        let r = self.write_to(ALT_ADDR, &mut p);
        if debug() {
            eprintln!("[ddc] écriture (adresse 0x50) VCP {code:#04x} = {value:#06x} : {}", if r.is_ok() { "acceptée par le bus" } else { "refusée" });
        }
        sleep(MESSAGE_GAP);
        r
    }

    /// Lit une valeur VCP (opcode 0x01 « Get VCP Feature »).
    pub fn read(&self, code: u8) -> Result<VcpValue> {
        let api = iokit::av_api().ok_or(Error::Unsupported("IOAVService indisponible"))?;
        let _g = self.lock.lock().unwrap();
        let mut last = Error::Ddc("pas de réponse".into());
        for _ in 0..READ_RETRIES {
            let mut p = [0x82, 0x01, code, 0];
            p[3] = checksum(0x6E ^ 0x51, &p[..3]);
            if let Err(e) = self.write_raw(&mut p) {
                last = e;
                continue;
            }
            sleep(READ_DELAY);
            let mut reply = [0u8; 11];
            let r = unsafe {
                (api.read_i2c)(self.service, I2C_ADDR, DATA_ADDR, reply.as_mut_ptr(), reply.len() as u32)
            };
            if r != 0 {
                last = Error::Ddc(format!("lecture I2C : code {r:#x}"));
                continue;
            }
            if debug() {
                eprintln!("[ddc] VCP {code:#04x} réponse brute : {reply:02x?}");
            }
            match parse_reply(code, &reply) {
                Ok(v) => return Ok(v),
                Err(e) => last = e,
            }
        }
        Err(last)
    }
}

impl DdcService {
    /// Lit la chaîne de capacités MCCS (opcode 0xF3), par fragments de 32 octets.
    /// Exemple : `(prot(monitor)type(lcd)vcp(02 10 12 14(05 08 0B) 60(0F 11))mccs_ver(2.2))`.
    pub fn capabilities(&self) -> Result<String> {
        let api = iokit::av_api().ok_or(Error::Unsupported("IOAVService indisponible"))?;
        let _g = self.lock.lock().unwrap();
        let mut out = Vec::new();
        let mut offset: u16 = 0;
        let mut failures = 0;
        while out.len() < 4096 {
            let mut p = [0x83, 0xF3, (offset >> 8) as u8, offset as u8, 0];
            p[4] = checksum(0x6E ^ 0x51, &p[..4]);
            self.write_raw(&mut p)?;
            sleep(READ_DELAY);
            let mut r = [0u8; 38];
            let rc = unsafe { (api.read_i2c)(self.service, I2C_ADDR, DATA_ADDR, r.as_mut_ptr(), r.len() as u32) };
            let len = (r[1] & 0x7F) as usize;
            let valid = rc == 0 && r[2] == 0xE3 && u16::from_be_bytes([r[3], r[4]]) == offset && len >= 3 && len <= 35;
            if !valid {
                failures += 1;
                if failures > READ_RETRIES {
                    return Err(Error::Ddc("capacités illisibles".into()));
                }
                continue;
            }
            let n = len - 3;
            if n == 0 {
                break;
            }
            out.extend_from_slice(&r[5..5 + n]);
            offset += n as u16;
        }
        Ok(caps_text(&out))
    }
}

/// Codes VCP annoncés dans une chaîne de capacités, avec leurs valeurs permises
/// (vide pour un réglage continu).
/// Texte des capacités, coupé au premier nul : certains écrans complètent le
/// dernier fragment par un nul suivi d'octets parasites.
fn caps_text(raw: &[u8]) -> String {
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).trim().to_string()
}

pub fn parse_vcp_list(caps: &str) -> std::collections::BTreeMap<u8, Vec<u8>> {
    let mut map = std::collections::BTreeMap::new();
    let lower = caps.to_ascii_lowercase();
    let Some(start) = lower.find("vcp(") else { return map };
    let body = &caps[start + 4..];
    let mut depth = 0usize;
    let mut current: Option<u8> = None;
    let mut token = String::new();
    let flush = |token: &mut String, depth: usize, current: &mut Option<u8>, map: &mut std::collections::BTreeMap<u8, Vec<u8>>| {
        if let Ok(v) = u8::from_str_radix(token.trim(), 16) {
            if depth == 0 {
                map.entry(v).or_insert_with(Vec::new);
                *current = Some(v);
            } else if let Some(c) = *current {
                map.entry(c).or_insert_with(Vec::new).push(v);
            }
        }
        token.clear();
    };
    for ch in body.chars() {
        match ch {
            '(' => {
                flush(&mut token, depth, &mut current, &mut map);
                depth += 1;
            }
            ')' => {
                flush(&mut token, depth, &mut current, &mut map);
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            c if c.is_ascii_hexdigit() => token.push(c),
            _ => flush(&mut token, depth, &mut current, &mut map),
        }
    }
    map
}

/// Réponse attendue : `[0x6E, 0x88, 0x02, résultat, code, type, max_hi, max_lo, cur_hi, cur_lo, chk]`.
fn parse_reply(code: u8, r: &[u8; 11]) -> Result<VcpValue> {
    if checksum(0x50, &r[..10]) != r[10] {
        return Err(Error::Ddc("checksum invalide".into()));
    }
    if r[2] != 0x02 || r[4] != code {
        return Err(Error::Ddc("réponse inattendue".into()));
    }
    if r[3] != 0 {
        return Err(Error::Ddc(format!("code VCP {code:#04x} non supporté par l'écran")));
    }
    Ok(VcpValue {
        max: u16::from_be_bytes([r[6], r[7]]),
        current: u16::from_be_bytes([r[8], r[9]]),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn caps_text_stops_at_nul() {
        assert_eq!(super::caps_text(b"(vcp(10 12))\0\xff\xfe"), "(vcp(10 12))");
        assert_eq!(super::caps_text(b" (vcp(10)) "), "(vcp(10))");
    }

    use super::*;

    #[test]
    fn parse_valid_reply() {
        let mut r = [0x6E, 0x88, 0x02, 0x00, 0x10, 0x00, 0x00, 0x64, 0x00, 0x32, 0];
        r[10] = checksum(0x50, &r[..10]);
        assert_eq!(parse_reply(0x10, &r).unwrap(), VcpValue { current: 50, max: 100 });
    }

    #[test]
    fn parses_vcp_list() {
        let caps = "(prot(monitor)type(lcd)vcp(02 10 12 14(05 08 0B) 60(0F 11 12) 62 DC(00 03 05))mccs_ver(2.2))";
        let m = parse_vcp_list(caps);
        assert_eq!(m[&0x10], Vec::<u8>::new());
        assert_eq!(m[&0x14], vec![0x05, 0x08, 0x0B]);
        assert_eq!(m[&0x60], vec![0x0F, 0x11, 0x12]);
        assert_eq!(m[&0xDC], vec![0x00, 0x03, 0x05]);
        assert!(m.contains_key(&0x62));
    }

    #[test]
    fn reject_bad_checksum() {
        let r = [0x6E, 0x88, 0x02, 0x00, 0x10, 0x00, 0x00, 0x64, 0x00, 0x32, 0xFF];
        assert!(parse_reply(0x10, &r).is_err());
    }
}
