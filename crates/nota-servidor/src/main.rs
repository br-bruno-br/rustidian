//! `nota-servidor`: o servidor de sincronização.
//!
//! ```text
//! nota-servidor serve --dados <pasta> [--escutar <ip:porta>] [--permitir-todas-interfaces]
//! nota-servidor token novo <aparelho> --dados <pasta>
//! ```
//!
//! Sem `--escutar`, usa `escutar` de `<pasta>/config.json`. O token é impresso uma vez só.
#![forbid(unsafe_code)]

use std::path::PathBuf;
use std::process::ExitCode;

use nota_servidor::{ConfigServidor, ler_config, servir, tokens};

const AJUDA: &str = "uso:\n  nota-servidor serve --dados <pasta> [--escutar <ip:porta>] [--permitir-todas-interfaces]\n  nota-servidor token novo <aparelho> --dados <pasta>\n";

#[derive(Default)]
struct Opcoes {
    dados: Option<PathBuf>,
    escutar: Option<String>,
    todas: bool,
    posicionais: Vec<String>,
}

fn analisar(args: &[String]) -> Result<Opcoes, String> {
    let mut o = Opcoes::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dados" => o.dados = Some(it.next().ok_or("falta o valor de --dados")?.into()),
            "--escutar" => o.escutar = Some(it.next().ok_or("falta o valor de --escutar")?.clone()),
            "--permitir-todas-interfaces" => o.todas = true,
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with("--") => return Err(format!("opção desconhecida: {s}")),
            s => o.posicionais.push(s.to_string()),
        }
    }
    Ok(o)
}

fn executar(args: &[String]) -> Result<(), String> {
    let o = analisar(args)?;
    let dados = o.dados.clone().ok_or("falta --dados <pasta>")?;
    match o.posicionais.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["serve"] => {
            let escutar = match o.escutar {
                Some(e) => e,
                None => ler_config(&dados)?
                    .and_then(|c| c.escutar)
                    .ok_or("falta --escutar <ip:porta> (ou \"escutar\" em dados/config.json)")?,
            };
            let escutar = escutar
                .parse()
                .map_err(|e| format!("endereço inválido '{escutar}': {e}"))?;
            servir(ConfigServidor {
                dados,
                escutar,
                permitir_todas_interfaces: o.todas,
            })
        }
        ["token", "novo", aparelho] => {
            let token = tokens::novo(&dados, aparelho).map_err(|e| e.to_string())?;
            println!("Token do aparelho '{aparelho}' (aparece só desta vez, guarde agora):");
            println!("{token}");
            Ok(())
        }
        _ => Err(String::new()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match executar(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(m) => {
            if !m.is_empty() {
                eprintln!("erro: {m}");
            }
            eprint!("{AJUDA}");
            ExitCode::from(2)
        }
    }
}
