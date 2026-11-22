use clap::{Parser, Subcommand};
use dusk_core::{list_displays, Display};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "dusk", version, about = "Contrôle matériel de la luminosité des écrans")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Liste les écrans et leur canal de contrôle
    List,
    /// Lit la luminosité (index de `list`, ou `all`)
    Get { target: String },
    /// Règle la luminosité en pourcentage
    Set { target: String, percent: f32 },
    /// Affiche ce que l'écran sait faire (chaîne de capacités)
    Caps { target: usize },
    /// Lit ou écrit un code VCP brut (ex. `dusk vcp 0 0x12 70`)
    Vcp {
        target: usize,
        #[arg(value_parser = parse_u8)]
        code: u8,
        value: Option<u16>,
    },
}

fn parse_u8(s: &str) -> Result<u8, String> {
    let s = s.trim();
    let r = match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u8::from_str_radix(h, 16),
        None => s.parse(),
    };
    r.map_err(|e| e.to_string())
}

fn select<'a>(displays: &'a [Display], target: &str) -> Result<Vec<(usize, &'a Display)>, String> {
    if target == "all" {
        return Ok(displays.iter().enumerate().filter(|(_, d)| d.has_hardware()).collect());
    }
    let i: usize = target.parse().map_err(|_| format!("cible invalide : {target}"))?;
    displays.get(i).map(|d| vec![(i, d)]).ok_or_else(|| format!("pas d'écran n°{i}"))
}

fn run(cli: Cli) -> Result<(), String> {
    let displays = list_displays();
    match cli.cmd {
        Cmd::List => {
            for (i, d) in displays.iter().enumerate() {
                println!("{i}  {:<20} {:<16} id={} clé={}", d.name, d.backend_name(), d.id, d.key());
            }
        }
        Cmd::Get { target } => {
            for (i, d) in select(&displays, &target)? {
                match d.brightness() {
                    Ok(v) => println!("{i}  {:<20} {v:.0} %", d.name),
                    Err(e) => println!("{i}  {:<20} {e}", d.name),
                }
            }
        }
        Cmd::Set { target, percent } => {
            for (i, d) in select(&displays, &target)? {
                let max = d.ddc().and_then(|s| s.read(0x10).ok()).map_or(100, |v| v.max.max(1));
                d.set_brightness(percent, max).map_err(|e| format!("{i} {} : {e}", d.name))?;
            }
        }
        Cmd::Caps { target } => {
            let d = displays.get(target).ok_or("écran inconnu")?;
            let s = d.ddc().ok_or("cet écran n'est pas piloté en DDC")?;
            let caps = s.capabilities().map_err(|e| e.to_string())?;
            println!("{caps}\n");
            for (code, values) in dusk_core::ddc::parse_vcp_list(&caps) {
                let v: Vec<String> = values.iter().map(|v| format!("{v:02X}")).collect();
                println!("{code:02X}  {}", v.join(" "));
            }
        }
        Cmd::Vcp { target, code, value } => {
            let d = displays.get(target).ok_or("écran inconnu")?;
            let s = d.ddc().ok_or("cet écran n'est pas piloté en DDC")?;
            match value {
                Some(v) => s.write(code, v).map_err(|e| e.to_string())?,
                None => {
                    let v = s.read(code).map_err(|e| e.to_string())?;
                    println!("VCP {code:#04x} = {} / {}", v.current, v.max);
                }
            }
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("dusk : {e}");
            ExitCode::FAILURE
        }
    }
}
