use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;

use crate::risk::{risk_check_internal, RiskCheckRequest, RiskState};

const ONEINCH_NATIVE_ETH: &str = "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE";

#[derive(Debug, Deserialize, Clone)]
pub struct ExecutePrepareRequest {
    pub chain: String,
    pub sell: String, // "ETH" or 0x...
    pub buy: String,  // "ETH" or 0x...
    pub sell_amount: String,
    pub slippage_bps: u32,
    pub recipient: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct PreparedTx {
    pub to: String,
    pub data: String,
    pub value: String,
    pub gas: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ExecutePrepareResponse {
    pub ok: bool,
    pub venue: String,
    pub chain: String,
    pub sell: String,
    pub buy: String,
    pub sell_amount: String,
    pub slippage_bps: u32,
    pub recipient: String,
    pub tx: PreparedTx,
    pub quote_out: Option<String>,
    pub note: String,
}

fn normalize_chain_id(chain: &str) -> Option<u32> {
    match chain.trim().to_lowercase().as_str() {
        "ethereum" | "eth" => Some(1),
        "polygon" | "matic" => Some(137),
        "arbitrum" | "arb" => Some(42161),
        "base" => Some(8453),
        _ => None,
    }
}

fn is_hex_addr(s: &str) -> bool {
    let s = s.trim();
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn normalize_token_for_1inch(t: &str) -> Result<String, String> {
    if t.eq_ignore_ascii_case("eth") {
        Ok(ONEINCH_NATIVE_ETH.to_string())
    } else if is_hex_addr(t) {
        Ok(t.to_string())
    } else {
        Err("Token must be ETH or 0x address".into())
    }
}

fn client_ip(req: &HttpRequest) -> IpAddr {
    req.peer_addr()
        .map(|p| p.ip())
        .unwrap_or_else(|| "127.0.0.1".parse().unwrap())
}

pub async fn prepare_execute_internal(
    state: &RiskState,
    ip: IpAddr,
    body: &ExecutePrepareRequest,
) -> Result<ExecutePrepareResponse, String> {
    // Risk check first
    let risk_req = RiskCheckRequest {
        chain: body.chain.clone(),
        sell: body.sell.clone(),
        buy: body.buy.clone(),
        sell_amount: body.sell_amount.clone(),
        slippage_bps: body.slippage_bps,
        recipient: body.recipient.clone(),
    };

    let risk = risk_check_internal(state, ip, &risk_req);
    if !risk.ok {
        return Err(format!("Risk check failed: {}", risk.reasons.join("; ")));
    }

    let chain_id =
        normalize_chain_id(&body.chain).ok_or_else(|| "Unsupported chain".to_string())?;

    let api_key = std::env::var("ONEINCH_API_KEY").unwrap_or_default();
    if api_key.trim().is_empty() {
        return Err("ONEINCH_API_KEY missing".into());
    }

    let sell_addr = normalize_token_for_1inch(&body.sell)?;
    let buy_addr = normalize_token_for_1inch(&body.buy)?;

    let url = format!("https://api.1inch.dev/swap/v5.2/{}/swap", chain_id);
    let client = reqwest::Client::new();

    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .query(&[
            ("fromTokenAddress", sell_addr.as_str()),
            ("toTokenAddress", buy_addr.as_str()),
            ("amount", body.sell_amount.as_str()),
            ("fromAddress", body.recipient.as_str()),
            (
                "slippage",
                &format!("{:.2}", body.slippage_bps as f64 / 100.0),
            ),
            ("disableEstimate", "true"),
        ])
        .send()
        .await
        .map_err(|e| format!("1inch request failed: {}", e))?;

    let status = resp.status();
    let raw = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("1inch error {}: {}", status.as_u16(), raw));
    }

    let json: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| "Invalid 1inch JSON".to_string())?;

    let tx = &json["tx"];
    let to = tx["to"].as_str().unwrap_or("").to_string();
    let data = tx["data"].as_str().unwrap_or("").to_string();
    let value = tx["value"].as_str().unwrap_or("0").to_string();
    let gas = tx["gas"].as_u64().map(|g| g.to_string());
    let quote_out = json["toTokenAmount"].as_str().map(|s| s.to_string());

    if to.is_empty() || data.is_empty() {
        return Err("Malformed 1inch tx".into());
    }

    let note: String = if body.sell.eq_ignore_ascii_case("ETH") {
        "Prepared tx only. Native ETH swap. No approval needed.".to_string()
    } else {
        "Prepared tx only. ERC20 swap - ensure allowance is sufficient (see /api/erc20/allowance)."
            .to_string()
    };

    Ok(ExecutePrepareResponse {
        ok: true,
        venue: "1inch".into(),
        chain: body.chain.clone(),
        sell: body.sell.clone(),
        buy: body.buy.clone(),
        sell_amount: body.sell_amount.clone(),
        slippage_bps: body.slippage_bps,
        recipient: body.recipient.clone(),
        tx: PreparedTx {
            to,
            data,
            value,
            gas,
        },
        quote_out,
        note,
    })
}

pub async fn prepare_execute(
    req: HttpRequest,
    state: web::Data<RiskState>,
    body: web::Json<ExecutePrepareRequest>,
) -> impl Responder {
    let ip = client_ip(&req);
    match prepare_execute_internal(&state, ip, &body).await {
        Ok(v) => HttpResponse::Ok().json(v),
        Err(e) => HttpResponse::BadRequest().json(serde_json::json!({ "error": e })),
    }
}
