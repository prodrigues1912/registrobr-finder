use anyhow::{Context, Result};
use clap::Parser;
use futures::stream::{self, StreamExt};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const AVAIL_API_URL: &str = "https://registro.br/v2/ajax/avail/raw/";

#[derive(Parser, Debug)]
#[command(name = "registrobr-finder")]
#[command(about = "Verifica disponibilidade de domínios .BR via RDAP")]
struct Args {
    /// Número de caracteres (2 ou 3)
    #[arg(short, long, default_value_t = 2)]
    digits: u8,

    /// Número de requisições paralelas
    #[arg(short, long, default_value_t = 20)]
    workers: usize,

    /// Timeout por requisição em segundos
    #[arg(short, long, default_value_t = 10)]
    timeout: u64,

    /// Sufixo do domínio
    #[arg(short, long, default_value = ".com.br")]
    suffix: String,

    /// Apenas letras (sem números)
    #[arg(long)]
    letters: bool,

    /// Apenas números (sem letras)
    #[arg(long)]
    numbers: bool,

    /// Arquivo para salvar domínios disponíveis
    #[arg(short, long)]
    output: Option<String>,

    /// Verificar domínio(s) específico(s), separados por vírgula
    #[arg(short, long)]
    check: Option<String>,

    /// Mostra todos os domínios verificados
    #[arg(short, long)]
    verbose: bool,
}

/// Resposta da API de disponibilidade do Registro.br
/// status: 0 = disponível, 2 = registrado, 3 = em processo, 4 = indisponível
#[derive(Debug, Deserialize)]
struct AvailResponse {
    status: i32,
    fqdn: String,
    #[serde(rename = "publication-status")]
    publication_status: Option<String>,
    #[serde(rename = "expires-at")]
    expires_at: Option<String>,
}

#[derive(Debug, Clone)]
struct DomainResult {
    domain: String,
    available: bool,
    status: Option<String>,
    error: Option<String>,
}

fn generate_combinations(length: u8, letters_only: bool, numbers_only: bool) -> Vec<String> {
    let chars: Vec<char> = if numbers_only {
        "0123456789".chars().collect()
    } else if letters_only {
        "abcdefghijklmnopqrstuvwxyz".chars().collect()
    } else {
        "abcdefghijklmnopqrstuvwxyz0123456789".chars().collect()
    };

    let mut combinations = Vec::new();
    let base = chars.len();
    let total = base.pow(length as u32);

    for i in 0..total {
        let mut combo = String::with_capacity(length as usize);
        let mut n = i;
        for _ in 0..length {
            combo.push(chars[n % base]);
            n /= base;
        }
        combinations.push(combo.chars().rev().collect());
    }

    combinations
}

async fn check_domain(client: &Client, domain: &str, suffix: &str) -> DomainResult {
    let full_domain = format!("{}{}", domain, suffix);
    let url = format!("{}{}", AVAIL_API_URL, full_domain);

    match client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")
        .send()
        .await
    {
        Ok(response) => {
            let status_code = response.status();

            if status_code == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return DomainResult {
                    domain: full_domain,
                    available: false,
                    status: None,
                    error: Some("rate limited".to_string()),
                };
            }

            if status_code.is_success() {
                match response.json::<AvailResponse>().await {
                    Ok(avail) => {
                        // status: 0 = disponível, 2 = registrado, 3 = em processo, 4 = indisponível
                        let available = avail.status == 0;
                        let status_str = match avail.status {
                            0 => "disponível".to_string(),
                            2 => {
                                if let Some(expires) = avail.expires_at {
                                    format!("registrado (expira: {})", expires.split('T').next().unwrap_or(&expires))
                                } else {
                                    "registrado".to_string()
                                }
                            }
                            3 => "em processo".to_string(),
                            4 => "indisponível".to_string(),
                            _ => format!("status {}", avail.status),
                        };
                        DomainResult {
                            domain: full_domain,
                            available,
                            status: Some(status_str),
                            error: None,
                        }
                    }
                    Err(e) => DomainResult {
                        domain: full_domain,
                        available: false,
                        status: None,
                        error: Some(format!("parse error: {}", e)),
                    },
                }
            } else {
                DomainResult {
                    domain: full_domain,
                    available: false,
                    status: None,
                    error: Some(format!("HTTP {}", status_code)),
                }
            }
        }
        Err(e) => DomainResult {
            domain: full_domain,
            available: false,
            status: None,
            error: Some(e.to_string()),
        },
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!("Verificador de Domínios .BR");
    println!("==============================");
    println!(
        "Sufixo: {} | Workers: {} | Timeout: {}s\n",
        args.suffix, args.workers, args.timeout
    );

    let domains: Vec<String> = if let Some(ref check) = args.check {
        check.split(',').map(|s| s.trim().to_string()).collect()
    } else {
        generate_combinations(args.digits, args.letters, args.numbers)
    };

    println!("Total de domínios a verificar: {}\n", domains.len());

    let client = Client::builder()
        .timeout(Duration::from_secs(args.timeout))
        .build()
        .context("Falha ao criar cliente HTTP")?;

    let progress = ProgressBar::new(domains.len() as u64);
    progress.set_style(
        ProgressStyle::default_bar()
            .template(
                "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) | {msg}",
            )?
            .progress_chars("##-"),
    );

    let available_count = Arc::new(AtomicUsize::new(0));
    let error_count = Arc::new(AtomicUsize::new(0));
    let available_domains = Arc::new(tokio::sync::Mutex::new(Vec::new()));

    let suffix = args.suffix.clone();
    let verbose = args.verbose;

    let results: Vec<DomainResult> = stream::iter(domains)
        .map(|domain| {
            let client = client.clone();
            let suffix = suffix.clone();
            let progress = progress.clone();
            let available_count = available_count.clone();
            let error_count = error_count.clone();
            let available_domains = available_domains.clone();

            async move {
                let result = check_domain(&client, &domain, &suffix).await;

                if result.available {
                    available_count.fetch_add(1, Ordering::Relaxed);
                    let mut domains = available_domains.lock().await;
                    domains.push(result.domain.clone());
                    progress.println(format!("DISPONIVEL: {}", result.domain));
                } else if result.error.is_some() {
                    error_count.fetch_add(1, Ordering::Relaxed);
                    if verbose {
                        progress.println(format!(
                            "   ERRO: {} ({})",
                            result.domain,
                            result.error.as_ref().unwrap()
                        ));
                    }
                } else if verbose {
                    progress.println(format!(
                        "   REGISTRADO: {} ({})",
                        result.domain,
                        result.status.as_ref().unwrap_or(&"registrado".to_string())
                    ));
                }

                progress.inc(1);
                progress.set_message(format!(
                    "{} disponiveis",
                    available_count.load(Ordering::Relaxed)
                ));

                result
            }
        })
        .buffer_unordered(args.workers)
        .collect()
        .await;

    progress.finish_with_message(format!(
        "{} disponiveis, {} erros",
        available_count.load(Ordering::Relaxed),
        error_count.load(Ordering::Relaxed)
    ));

    // Resumo final
    let available: Vec<_> = results.iter().filter(|r| r.available).collect();

    println!("\n==============================");
    println!("RESUMO");
    println!("==============================");
    println!("Total verificado: {}", results.len());
    println!("Disponíveis: {}", available.len());
    println!("Erros: {}", error_count.load(Ordering::Relaxed));

    if !available.is_empty() {
        println!("\nDOMÍNIOS DISPONÍVEIS:");
        for d in &available {
            println!("   - {}", d.domain);
        }
    }

    // Salva em arquivo se especificado
    if let Some(ref output_file) = args.output {
        if !available.is_empty() {
            let file = File::create(output_file)
                .with_context(|| format!("Falha ao criar arquivo {}", output_file))?;
            let mut writer = BufWriter::new(file);

            for d in &available {
                writeln!(writer, "{}", d.domain)?;
            }

            println!("\nResultados salvos em: {}", output_file);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_combinations_2_digits_numbers_only() {
        let combos = generate_combinations(2, false, true);
        assert_eq!(combos.len(), 100); // 10^2
        assert!(combos.contains(&"00".to_string()));
        assert!(combos.contains(&"99".to_string()));
        assert!(combos.contains(&"42".to_string()));
    }

    #[test]
    fn test_generate_combinations_2_digits_letters_only() {
        let combos = generate_combinations(2, true, false);
        assert_eq!(combos.len(), 676); // 26^2
        assert!(combos.contains(&"aa".to_string()));
        assert!(combos.contains(&"zz".to_string()));
        assert!(combos.contains(&"ab".to_string()));
    }

    #[test]
    fn test_generate_combinations_2_digits_alphanumeric() {
        let combos = generate_combinations(2, false, false);
        assert_eq!(combos.len(), 1296); // 36^2
        assert!(combos.contains(&"aa".to_string()));
        assert!(combos.contains(&"00".to_string()));
        assert!(combos.contains(&"a1".to_string()));
        assert!(combos.contains(&"z9".to_string()));
    }

    #[test]
    fn test_generate_combinations_3_digits_numbers_only() {
        let combos = generate_combinations(3, false, true);
        assert_eq!(combos.len(), 1000); // 10^3
        assert!(combos.contains(&"000".to_string()));
        assert!(combos.contains(&"999".to_string()));
        assert!(combos.contains(&"123".to_string()));
    }

    #[test]
    fn test_generate_combinations_3_digits_letters_only() {
        let combos = generate_combinations(3, true, false);
        assert_eq!(combos.len(), 17576); // 26^3
        assert!(combos.contains(&"aaa".to_string()));
        assert!(combos.contains(&"zzz".to_string()));
        assert!(combos.contains(&"abc".to_string()));
    }

    #[test]
    fn test_generate_combinations_3_digits_alphanumeric() {
        let combos = generate_combinations(3, false, false);
        assert_eq!(combos.len(), 46656); // 36^3
    }
}
