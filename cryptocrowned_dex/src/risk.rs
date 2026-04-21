// src/risk.rs
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const ONEINCH_NATIVE_ETH: &str = "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE";

fn lc(s: &str) -> String {
    s.trim().to_lowercase()
}

fn is_hex_addr(s: &str) -> bool {
    let s = s.trim();
    if s.len() != 42 {
        return false;
    }
    if !s.starts_with("0x") {
        return false;
    }
    s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_u128(s: &str) -> Option<u128> {
    s.trim().parse::<u128>().ok()
}

fn client_ip(req: &HttpRequest) -> IpAddr {
    req.peer_addr()
        .map(|p| p.ip())
        .unwrap_or_else(|| "127.0.0.1".parse().unwrap())
}

/// Accepts:
/// - "ETH" / "eth" -> 1inch native ETH address
/// - 0x... address -> same (lowercased)
fn normalize_token(s: &str) -> Option<String> {
    let t = s.trim();
    if t.eq_ignore_ascii_case("eth") {
        return Some(lc(ONEINCH_NATIVE_ETH));
    }
    if is_hex_addr(t) {
        return Some(lc(t));
    }
    None
}

#[derive(Debug, Deserialize, Clone)]
pub struct RiskCheckRequest {
    pub chain: String,
    pub sell: String,
    pub buy: String,
    pub sell_amount: String,
    pub slippage_bps: u32,
    pub recipient: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct RiskLimits {
    pub max_slippage_bps: u32,
    pub max_sell_amount_wei: String,
    pub rate_limit_per_min_wallet: u32,
    pub rate_limit_per_min_ip: u32,
    pub allowed_chains: Vec<String>,
    pub allowed_tokens: Vec<String>,
    pub kill_switch: bool,
}

#[derive(Debug, Serialize, Clone)]
pub struct RiskCheckResponse {
    pub ok: bool,
    pub reasons: Vec<String>,
    pub limits: RiskLimits,
}

#[derive(Clone)]
pub struct RiskState {
    // Config
    pub kill_switch: bool,
    pub max_slippage_bps: u32,
    pub max_sell_amount_wei: u128,
    pub allowed_chains: HashSet<String>,
    pub allowed_tokens: HashSet<String>,

    // Rate limiting
    pub rate_limit_wallet_per_min: u32,
    pub rate_limit_ip_per_min: u32,
    pub per_wallet: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
    pub per_ip: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
}

impl RiskState {
    pub fn from_env() -> Self {
        let kill_switch = std::env::var("KILL_SWITCH")
            .unwrap_or_else(|_| "false".into())
            .to_lowercase()
            == "true";

        let max_slippage_bps = std::env::var("MAX_SLIPPAGE_BPS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(100);

        let max_sell_amount_wei = std::env::var("MAX_SELL_AMOUNT_WEI")
            .ok()
            .and_then(|v| v.parse::<u128>().ok())
            .unwrap_or(5_000_000_000_000_000_000u128);

        let rate_limit_wallet_per_min = std::env::var("RATE_LIMIT_WALLET_PER_MIN")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(10);

        let rate_limit_ip_per_min = std::env::var("RATE_LIMIT_IP_PER_MIN")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(30);

        let allowed_chains_str =
            std::env::var("ALLOWED_CHAINS").unwrap_or_else(|_| "ethereum".into());

        let allowed_chains: HashSet<String> = allowed_chains_str
            .split(',')
            .map(lc)
            .filter(|s| !s.is_empty())
            .collect();

        // Defaults: ETH(native via 1inch), WETH, USDC
        let default_allowed = [
            ONEINCH_NATIVE_ETH,                           // native ETH
            "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", // WETH
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48", // USDC
        ]
        .join(",");

        let allowed_tokens_str =
            std::env::var("ALLOWED_TOKENS").unwrap_or_else(|_| default_allowed);

        let allowed_tokens: HashSet<String> = allowed_tokens_str
            .split(',')
            .map(lc)
            .filter(|s| !s.is_empty())
            .collect();

        Self {
            kill_switch,
            max_slippage_bps,
            max_sell_amount_wei,
            allowed_chains,
            allowed_tokens,
            rate_limit_wallet_per_min,
            rate_limit_ip_per_min,
            per_wallet: Arc::new(Mutex::new(HashMap::new())),
            per_ip: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn limits_view(&self) -> RiskLimits {
        RiskLimits {
            max_slippage_bps: self.max_slippage_bps,
            max_sell_amount_wei: self.max_sell_amount_wei.to_string(),
            rate_limit_per_min_wallet: self.rate_limit_wallet_per_min,
            rate_limit_per_min_ip: self.rate_limit_ip_per_min,
            allowed_chains: self.allowed_chains.iter().cloned().collect(),
            allowed_tokens: self.allowed_tokens.iter().cloned().collect(),
            kill_switch: self.kill_switch,
        }
    }
}

fn rate_limit_hit(map: &mut HashMap<String, (Instant, u32)>, key: String, limit: u32) -> bool {
    let now = Instant::now();
    let entry = map.entry(key).or_insert((now, 0));

    if now.duration_since(entry.0) > Duration::from_secs(60) {
        *entry = (now, 0);
    }

    entry.1 += 1;
    entry.1 > limit
}

pub fn risk_check_internal(
    state: &RiskState,
    ip: IpAddr,
    req: &RiskCheckRequest,
) -> RiskCheckResponse {
    let mut reasons: Vec<String> = vec![];

    if state.kill_switch {
        reasons.push("Trading is disabled (KILL_SWITCH=true)".into());
    }

    // Validate recipient
    if !is_hex_addr(&req.recipient) {
        reasons.push("Invalid recipient address".into());
    }

    // Chain allowlist
    let chain_lc = lc(&req.chain);
    if !state.allowed_chains.contains(&chain_lc) {
        reasons.push(format!("Chain not allowed: {}", req.chain));
    }

    // Token normalization + allowlist
    let sell_norm = normalize_token(&req.sell);
    let buy_norm = normalize_token(&req.buy);

    match sell_norm.as_deref() {
        Some(addr) => {
            if !state.allowed_tokens.contains(addr) {
                reasons.push("Sell token not allowed".into());
            }
        }
        None => reasons.push("Invalid sell token (use ETH or 0x... address)".into()),
    }

    match buy_norm.as_deref() {
        Some(addr) => {
            if !state.allowed_tokens.contains(addr) {
                reasons.push("Buy token not allowed".into());
            }
        }
        None => reasons.push("Invalid buy token (use ETH or 0x... address)".into()),
    }

    // Slippage
    if req.slippage_bps > state.max_slippage_bps {
        reasons.push(format!(
            "Slippage too high: {} bps (max {} bps)",
            req.slippage_bps, state.max_slippage_bps
        ));
    }

    // Amount cap
    match parse_u128(&req.sell_amount) {
        Some(a) => {
            if a == 0 {
                reasons.push("Sell amount must be > 0".into());
            }
            if a > state.max_sell_amount_wei {
                reasons.push(format!(
                    "Sell amount exceeds max cap: {} > {}",
                    a, state.max_sell_amount_wei
                ));
            }
        }
        None => reasons.push("sell_amount is not a valid integer".into()),
    }

    // Rate limits (wallet + IP)
    if is_hex_addr(&req.recipient) {
        let mut wallets = state.per_wallet.lock().unwrap();
        if rate_limit_hit(
            &mut wallets,
            lc(&req.recipient),
            state.rate_limit_wallet_per_min,
        ) {
            reasons.push("Rate limit exceeded for wallet".into());
        }
    }

    {
        let mut ips = state.per_ip.lock().unwrap();
        if rate_limit_hit(&mut ips, ip.to_string(), state.rate_limit_ip_per_min) {
            reasons.push("Rate limit exceeded for IP".into());
        }
    }

    RiskCheckResponse {
        ok: reasons.is_empty(),
        reasons,
        limits: state.limits_view(),
    }
}

pub async fn risk_check_handler(
    http_req: HttpRequest,
    state: web::Data<RiskState>,
    body: web::Json<RiskCheckRequest>,
) -> impl Responder {
    let ip = client_ip(&http_req);
    let res = risk_check_internal(&state, ip, &body);
    HttpResponse::Ok().json(res)
}
