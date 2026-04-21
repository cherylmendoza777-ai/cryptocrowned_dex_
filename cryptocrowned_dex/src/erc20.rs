use actix_web::{web, HttpResponse, Responder};
use ethers_core::abi::{encode, Token};
use ethers_core::types::{Address, U256};
use serde::{Deserialize, Serialize};

fn is_hex_addr(s: &str) -> bool {
    let s = s.trim();
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

fn parse_address(s: &str) -> Result<Address, String> {
    let t = s.trim();
    if !is_hex_addr(t) {
        return Err("Bad address".into());
    }
    t.parse::<Address>().map_err(|_| "Bad address".into())
}

fn parse_u256_dec(s: &str) -> Result<U256, String> {
    U256::from_dec_str(s.trim()).map_err(|_| "Bad integer".into())
}

fn selector(sig: &str) -> [u8; 4] {
    use ethers_core::utils::keccak256;
    let h = keccak256(sig.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

#[derive(Debug, Deserialize)]
pub struct AllowanceRequest {
    pub chain: String,
    pub token: String,
    pub owner: String,
    pub spender: String,
}

#[derive(Debug, Serialize)]
pub struct AllowanceResponse {
    pub ok: bool,
    pub chain: String,
    pub token: String,
    pub owner: String,
    pub spender: String,
    pub allowance: String,
    pub note: String,
}

#[derive(Debug, Deserialize)]
pub struct ApprovePrepareRequest {
    pub chain: String,
    pub token: String,
    pub owner: String,
    pub spender: String,
    pub amount: String,
}

#[derive(Debug, Serialize)]
pub struct PreparedTx {
    pub to: String,
    pub data: String,
    pub value: String,
    pub gas: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApprovePrepareResponse {
    pub ok: bool,
    pub chain: String,
    pub token: String,
    pub owner: String,
    pub spender: String,
    pub amount: String,
    pub tx: PreparedTx,
    pub note: String,
}

fn normalize_chain_id(chain: &str) -> Option<u64> {
    match chain.to_lowercase().as_str() {
        "ethereum" | "eth" => Some(1),
        "polygon" | "matic" => Some(137),
        "arbitrum" | "arb" => Some(42161),
        "base" => Some(8453),
        _ => None,
    }
}

pub async fn allowance_handler(body: web::Json<AllowanceRequest>) -> impl Responder {
    let chain_id = match normalize_chain_id(&body.chain) {
        Some(id) => id,
        None => return HttpResponse::BadRequest().json(serde_json::json!({"error":"Unsupported chain"})),
    };
    if chain_id != 1 {
        return HttpResponse::BadRequest().json(serde_json::json!({"error":"Only ethereum supported in this MVP"}));
    }
    let rpc_url = std::env::var("ETH_RPC_URL").unwrap_or_default();
    if rpc_url.trim().is_empty() {
        return HttpResponse::BadRequest().json(serde_json::json!({"error":"ETH_RPC_URL missing"}));
    }
    let token = match parse_address(&body.token) {
        Ok(a) => a,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({"error": e})),
    };
    let owner = match parse_address(&body.owner) {
        Ok(a) => a,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({"error": e})),
    };
    let spender = match parse_address(&body.spender) {
        Ok(a) => a,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({"error": e})),
    };
    let mut data = Vec::with_capacity(4 + 32 + 32);
    data.extend_from_slice(&selector("allowance(address,address)"));
    data.extend_from_slice(&encode(&[Token::Address(owner), Token::Address(spender)]));
    let data_hex = format!("0x{}", hex::encode(data));
    let payload = serde_json::json!({
        "jsonrpc":"2.0",
        "id": 1,
        "method":"eth_call",
        "params":[
          {"to": format!("{:#x}", token), "data": data_hex},
          "latest"
        ]
    });
    let client = reqwest::Client::new();
    let resp = match client.post(rpc_url).json(&payload).send().await {
        Ok(r) => r,
        Err(e) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("RPC request failed: {}", e)
            }))
        }
    };
    let v: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            return HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("RPC response parse failed: {}", e)
            }))
        }
    };
    if v.get("error").is_some() {
        return HttpResponse::BadRequest().json(serde_json::json!({
            "error":"RPC error","body":v
        }));
    }
    let result = v.get("result").and_then(|x| x.as_str()).unwrap_or("0x0");
    let raw = result.trim_start_matches("0x");
    let allowance = U256::from_str_radix(if raw.is_empty() { "0" } else { raw }, 16)
        .unwrap_or_else(|_| U256::zero());
    HttpResponse::Ok().json(AllowanceResponse {
        ok: true,
        chain: body.chain.clone(),
        token: body.token.clone(),
        owner: body.owner.clone(),
        spender: body.spender.clone(),
        allowance: allowance.to_string(),
        note: "Allowance is raw token units (uint256).".into(),
    })
}

pub async fn approve_prepare_handler(body: web::Json<ApprovePrepareRequest>) -> impl Responder {
    let chain_id = match normalize_chain_id(&body.chain) {
        Some(id) => id,
        None => return HttpResponse::BadRequest().json(serde_json::json!({"error":"Unsupported chain"})),
    };
    if chain_id != 1 {
        return HttpResponse::BadRequest().json(serde_json::json!({"error":"Only ethereum supported in this MVP"}));
    }
    let token = match parse_address(&body.token) {
        Ok(a) => a,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({"error": e})),
    };
    let spender = match parse_address(&body.spender) {
        Ok(a) => a,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({"error": e})),
    };
    let amount = match parse_u256_dec(&body.amount) {
        Ok(v) => v,
        Err(e) => return HttpResponse::BadRequest().json(serde_json::json!({"error": e})),
    };
    let mut data = Vec::with_capacity(4 + 32 + 32);
    data.extend_from_slice(&selector("approve(address,uint256)"));
    data.extend_from_slice(&encode(&[Token::Address(spender), Token::Uint(amount)]));
    let data_hex = format!("0x{}", hex::encode(data));
    HttpResponse::Ok().json(ApprovePrepareResponse {
        ok: true,
        chain: body.chain.clone(),
        token: body.token.clone(),
        owner: body.owner.clone(),
        spender: body.spender.clone(),
        amount: body.amount.clone(),
        tx: PreparedTx {
            to: format!("{:#x}", token),
            data: data_hex,
            value: "0".into(),
            gas: None,
        },
        note: "Prepared approve tx only. Not broadcast.".into(),
    })
}
