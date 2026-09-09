use crate::error::{MinerError, Result};
use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct HttpClient {
    #[allow(dead_code)]
    timeout: u64,
    engine_index: std::sync::Arc<AtomicUsize>,
    proxy_index: std::sync::Arc<AtomicUsize>,
    proxies: Vec<String>,
    engine_health: std::sync::Arc<Mutex<HashMap<String, EngineState>>>,
}

#[derive(Debug, Clone, Default)]
struct EngineState {
    consecutive_fails: u32,
    disabled_until: Option<Instant>,
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub text: String,
    pub elapsed: Duration,
    pub url: String,
    pub engine: String,
}

// Engine status (Sep 2026): startpage serves Anubis bot-challenges,
// swisscows returns a JS shell with no HTML results, ecosia rate-limits,
// qwant API 403s, searx/mojeek return no results.
// brave/bing/yandex return parseable HTML results.
const ENGINES: &[&str] = &["brave", "bing", "yandex"];
/// After this many consecutive failures an engine cools down...
const ENGINE_FAIL_THRESHOLD: u32 = 3;
/// ...for this long, so rate-limited engines recover instead of being hammered.
const ENGINE_COOLDOWN: Duration = Duration::from_secs(30 * 60);

impl HttpClient {
    pub fn new(proxy_pool: std::sync::Arc<crate::proxy::ProxyPool>, timeout: u64) -> Result<Self> {
        // Extract proxy URLs from the pool
        let proxies = proxy_pool.get_proxy_urls();
        Ok(Self {
            timeout,
            engine_index: std::sync::Arc::new(AtomicUsize::new(0)),
            proxy_index: std::sync::Arc::new(AtomicUsize::new(0)),
            proxies,
            engine_health: std::sync::Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn next_engine(&self) -> String {
        // Prefer healthy engines; if all are cooling down, round-robin anyway
        // (better to retry than stall discovery entirely).
        let now = Instant::now();
        // Justified: a poisoned mutex here means an engine-panic mid-update;
        // proceeding with stale health data is preferable to propagating it.
        #[allow(clippy::unwrap_used)]
        let health = self.engine_health.lock().unwrap();
        let healthy: Vec<&str> = ENGINES
            .iter()
            .copied()
            .filter(|e| {
                health
                    .get(*e)
                    .and_then(|s| s.disabled_until)
                    .is_none_or(|until| now >= until)
            })
            .collect();
        drop(health);

        let pool: &[&str] = if healthy.is_empty() {
            ENGINES
        } else {
            &healthy
        };
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as usize;
        let idx = ((nanos >> 10) + self.engine_index.fetch_add(1, Ordering::Relaxed)) % pool.len();
        pool[idx].to_string()
    }

    fn record_engine_result(&self, engine: &str, ok: bool) {
        // Justified: same poisoning rationale as `next_engine`.
        #[allow(clippy::unwrap_used)]
        let mut health = self.engine_health.lock().unwrap();
        let state = health.entry(engine.to_string()).or_default();
        if ok {
            if state.consecutive_fails > 0 {
                info!("Engine {} recovered", engine);
            }
            state.consecutive_fails = 0;
            state.disabled_until = None;
        } else {
            state.consecutive_fails += 1;
            if state.consecutive_fails >= ENGINE_FAIL_THRESHOLD {
                state.disabled_until = Some(Instant::now() + ENGINE_COOLDOWN);
                warn!(
                    "Engine {} failed {}x, cooling down for {}m",
                    engine,
                    state.consecutive_fails,
                    ENGINE_COOLDOWN.as_secs() / 60
                );
            }
        }
    }

    fn next_proxy(&self) -> Option<String> {
        if self.proxies.is_empty() {
            return None;
        }
        let idx = self.proxy_index.fetch_add(1, Ordering::Relaxed) % self.proxies.len();
        Some(self.proxies[idx].clone())
    }

    pub async fn get(&self, url: &url::Url) -> Result<Response> {
        let start = std::time::Instant::now();
        let url_str = url.to_string();
        let url_clone = url_str.clone();
        let fetch_path = Self::find_fetch_py();
        let proxy = self.next_proxy();

        let output = tokio::time::timeout(
            Duration::from_secs(60),
            tokio::task::spawn_blocking(move || {
                let mut cmd = Command::new("python3");
                cmd.arg(&fetch_path).arg(&url_clone);
                if let Some(ref p) = proxy {
                    cmd.arg("--search-proxy").arg("direct").arg("").arg(p);
                }
                cmd.output()
            }),
        )
        .await
        .map_err(|_| MinerError::Http("timeout".to_string()))?
        .map_err(|e| MinerError::Http(e.to_string()))?
        .map_err(|e| MinerError::Http(e.to_string()))?;

        let elapsed = start.elapsed();
        if output.status.success() {
            Ok(Response {
                status: 200,
                text: String::from_utf8_lossy(&output.stdout).to_string(),
                elapsed,
                url: url_str,
                engine: "direct".to_string(),
            })
        } else {
            Err(MinerError::Http(format!(
                "Python: {}",
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }

    pub async fn search_engine(&self, engine: &str, query: &str) -> Result<Response> {
        let start = std::time::Instant::now();
        let fetch_path = Self::find_fetch_py();
        let engine_clone = engine.to_string();
        let query_clone = query.to_string();
        let url_display = format!("search://{}?q={}", engine, query);
        let proxy = self.next_proxy();

        let output = tokio::time::timeout(
            Duration::from_secs(60),
            tokio::task::spawn_blocking(move || {
                let mut cmd = Command::new("python3");
                cmd.arg(&fetch_path)
                    .arg("--search")
                    .arg(&engine_clone)
                    .arg(&query_clone);
                if let Some(ref p) = proxy {
                    cmd.arg("--search-proxy")
                        .arg(&engine_clone)
                        .arg(&query_clone)
                        .arg(p);
                }
                cmd.output()
            }),
        )
        .await
        .map_err(|_| MinerError::Http("timeout".to_string()))?
        .map_err(|e| MinerError::Http(e.to_string()))?
        .map_err(|e| MinerError::Http(e.to_string()))?;

        let elapsed = start.elapsed();
        if output.status.success() {
            Ok(Response {
                status: 200,
                text: String::from_utf8_lossy(&output.stdout).to_string(),
                elapsed,
                url: url_display,
                engine: engine.to_string(),
            })
        } else {
            Err(MinerError::Http(format!(
                "Python [{}]: {}",
                engine,
                String::from_utf8_lossy(&output.stderr)
            )))
        }
    }

    pub async fn search_next(&self, query: &str) -> Result<Response> {
        // Try up to 3 engines (all available) before giving up
        let mut last_err = MinerError::Http("All engines failed".to_string());
        for attempt in 0..3 {
            let engine = self.next_engine();
            match self.search_engine(&engine, query).await {
                Ok(response) => {
                    self.record_engine_result(&engine, true);
                    return Ok(response);
                }
                Err(e) => {
                    self.record_engine_result(&engine, false);
                    if attempt < 2 {
                        warn!(
                            "Engine {} failed (attempt {}/3): {}, trying next...",
                            engine,
                            attempt + 1,
                            e
                        );
                        // Small delay before trying next engine
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        last_err = e;
                        continue;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Err(last_err)
    }

    pub async fn verify_email(&self, email: &str) -> Result<bool> {
        let email_clone = email.to_string();
        let fetch_path = Self::find_fetch_py();
        let output = tokio::time::timeout(
            Duration::from_secs(15),
            tokio::task::spawn_blocking(move || {
                Command::new("python3")
                    .arg(&fetch_path)
                    .arg("--verify-email")
                    .arg(&email_clone)
                    .output()
            }),
        )
        .await
        .map_err(|_| MinerError::Http("timeout".to_string()))?
        .map_err(|e| MinerError::Http(e.to_string()))?
        .map_err(|e| MinerError::Http(e.to_string()))?;
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout).to_string();
            if let Ok(result) = serde_json::from_str::<serde_json::Value>(&text) {
                return Ok(result
                    .get("has_mx")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false));
            }
        }
        Ok(false)
    }

    fn find_fetch_py() -> String {
        Self::find_fetch_py_static()
    }

    pub fn find_fetch_py_static() -> String {
        // 1) explicit override, 2) next to the binary, 3) ./fetch.py
        if let Ok(p) = std::env::var("CONTACT_MINER_FETCH_PY") {
            return p;
        }
        let exe_path = std::env::current_exe().unwrap_or_default();
        let fetch_py = exe_path
            .parent()
            .map(|p| p.join("fetch.py"))
            .unwrap_or_else(|| std::path::PathBuf::from("fetch.py"));
        if fetch_py.exists() {
            return fetch_py.to_string_lossy().to_string();
        }
        let local = std::path::PathBuf::from("fetch.py");
        if local.exists() {
            return local.to_string_lossy().to_string();
        }
        "fetch.py".to_string()
    }
}
