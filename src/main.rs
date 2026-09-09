use clap::{Parser, Subcommand};
use contact_miner::http::HttpClient;
use contact_miner::parser::HtmlParser;
use contact_miner::proxy::ProxyPool;
use contact_miner::store::{Lead, LeadStore};
use contact_miner::validator::EmailValidator;
use contact_miner::verifier::EmailVerifier;
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "contact-miner", about = "Multi-engine web contact discovery")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to the SQLite store
    #[arg(long, default_value = "leads.db")]
    db: String,

    /// Optional HTTP proxy (http://user:pass@host:port)
    #[arg(long)]
    proxy: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Search engines for a query and harvest contacts from results
    Discover {
        query: String,
        #[arg(default_value = "10")]
        pages: i64,
    },
    /// Deep-crawl a profile page (Linktree/Beacons) for all contact channels
    DeepCrawl { url: String },
    /// Check MX deliverability for an email
    Verify { email: String },
    /// Export all leads as JSON
    Export,
    /// Show lead count
    Stats,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let store = match LeadStore::open(&cli.db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("store error: {e}");
            std::process::exit(1);
        }
    };

    match cli.command {
        Commands::Discover { query, pages } => discover(&store, &cli.proxy, &query, pages).await,
        Commands::DeepCrawl { url } => deep_crawl(&store, &url).await,
        Commands::Verify { email } => verify(&email).await,
        Commands::Export => match store.export_json() {
            Ok(json) => println!("{json}"),
            Err(e) => eprintln!("export error: {e}"),
        },
        Commands::Stats => match store.count() {
            Ok(n) => println!("{n} leads"),
            Err(e) => eprintln!("stats error: {e}"),
        },
    }
}

// CLI binary: fail fast at startup on construction errors.
#[allow(clippy::expect_used)]
async fn discover(store: &LeadStore, proxy: &Option<String>, query: &str, pages: i64) {
    let proxies: Vec<String> = proxy.clone().into_iter().collect();
    let pool = Arc::new(ProxyPool::new(&proxies));
    let http = Arc::new(HttpClient::new(pool, 60).expect("http client"));
    let parser = HtmlParser::new();
    let validator = EmailValidator::new(&[]);
    let verifier = EmailVerifier::new();

    let mut found = 0i64;
    for page in 0..pages.max(1) {
        let q = if page == 0 {
            query.to_string()
        } else {
            format!(
                "{query} -site: pinterest.com -site:facebook.com&first={}",
                page * 10
            )
        };
        match http.search_next(&q).await {
            Ok(resp) => {
                let results = parser.parse_search_results(&resp.text).unwrap_or_default();
                println!("[{}] {} results", resp.engine, results.len());
                for r in &results {
                    // Emails straight from the results page
                    for email in parser.extract_emails(&r.snippet) {
                        if !validator.validate(&email) {
                            continue;
                        }
                        if !verifier.verify(&email).await {
                            continue;
                        }
                        let _ = store.upsert(&Lead {
                            email: email.clone(),
                            name: Some(r.title.clone()),
                            website: Some(r.url.clone()),
                            phone: None,
                            whatsapp: None,
                            telegram: None,
                            signal: None,
                            source: Some(query.to_string()),
                        });
                        found += 1;
                        println!("  + {email}");
                    }
                    // Deep-crawl profile pages (they carry the channels)
                    if r.url.contains("linktr.ee/") || r.url.contains("beacons.ai/") {
                        harvest_deep(&http, store, &parser, &validator, &verifier, &r.url, query)
                            .await;
                    }
                }
            }
            Err(e) => eprintln!("search failed: {e}"),
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    println!("done: {found} new contact events (dedup in store)");
}

// CLI binary: fail fast at startup on construction errors.
#[allow(clippy::expect_used)]
async fn deep_crawl(store: &LeadStore, url: &str) {
    let http = Arc::new(HttpClient::new(Arc::new(ProxyPool::new(&[])), 60).expect("http client"));
    let parser = HtmlParser::new();
    let validator = EmailValidator::new(&[]);
    let verifier = EmailVerifier::new();
    harvest_deep(&http, store, &parser, &validator, &verifier, url, "manual").await;
}

async fn harvest_deep(
    _http: &Arc<HttpClient>,
    store: &LeadStore,
    _parser: &HtmlParser,
    validator: &EmailValidator,
    verifier: &EmailVerifier,
    url: &str,
    source: &str,
) {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        tokio::task::spawn_blocking({
            let fetch_py = HttpClient::find_fetch_py_static();
            let url = url.to_string();
            move || {
                std::process::Command::new("python3")
                    .arg(&fetch_py)
                    .arg("--deep-crawl")
                    .arg(&url)
                    .output()
            }
        }),
    )
    .await;

    let Ok(Ok(Ok(o))) = output else { return };
    if !o.status.success() {
        return;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&String::from_utf8_lossy(&o.stdout))
    else {
        return;
    };

    let arr = |k: &str| -> Vec<String> {
        v.get(k)
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    };
    let first = |k: &str| arr(k).first().cloned();
    let follower = v
        .get("follower_count")
        .and_then(|f| f.as_i64())
        .map(|f| f.to_string());

    for email in arr("emails") {
        if !validator.validate(&email) {
            continue;
        }
        if !verifier.verify(&email).await {
            continue;
        }
        let _ = store.upsert(&Lead {
            email: email.clone(),
            name: None,
            website: Some(url.to_string()),
            phone: first("phones"),
            whatsapp: first("whatsapp"),
            telegram: first("telegram"),
            signal: first("signal"),
            source: Some(format!("{source} (deep)")),
        });
        let extra = follower
            .clone()
            .map(|f| format!(" followers={f}"))
            .unwrap_or_default();
        println!("  + {email}{extra}");
    }
}

async fn verify(email: &str) {
    let verifier = EmailVerifier::new();
    let ok = verifier.verify(email).await;
    println!("{email}: MX {}", if ok { "OK" } else { "MISSING" });
}
