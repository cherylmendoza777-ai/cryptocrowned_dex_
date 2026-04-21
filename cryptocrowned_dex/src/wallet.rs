use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;


fn lc(s: &str) -> String {
    s.trim().to_lowercase()
}

fn is_hex_addr(s: &str) -> bool {
    let s = s.trim();
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletProfile {
    pub owner: String,
    pub smart_wallet: String,
    pub chain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPolicy {
    pub session_key: String,
    pub expiry: u64,
    pub max_value_wei_per_tx: String,
    pub max_calls_per_min: u32,
    pub allowed_targets: Vec<String>,
    pub allowed_selectors: Vec<String>,
}

#[derive(Clone)]
pub struct WalletState {
    pub profiles: Arc<Mutex<HashMap<String, WalletProfile>>>,
    pub sessions: Arc<Mutex<HashMap<String, SessionPolicy>>>,
}

impl WalletState {
    pub fn new() -> Self {
        Self {
            profiles: Arc::new(Mutex::new(HashMap::new())),
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RegisterWalletRequest {
    pub owner: String,
    pub smart_wallet: String,
    pub chain: String,
}

pub async fn wallet_register_handler(
    state: web::Data<WalletState>,
    body: web::Json<RegisterWalletRequest>,
) -> impl Responder {
    if !is_hex_addr(&body.owner) || !is_hex_addr(&body.smart_wallet) {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": "Bad address" }));
    }
    let owner = lc(&body.owner);
    let profile = WalletProfile {
        owner: owner.clone(),
        smart_wallet: lc(&body.smart_wallet),
        chain: lc(&body.chain),
    };
    state.profiles.lock().unwrap().insert(owner, profile);
    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

#[derive(Debug, Deserialize)]
pub struct SessionDraftRequest {
    pub owner: String,
    pub chain_id: u64,
    pub verifying_contract: String,
    pub policy: SessionPolicy,
}

#[derive(Debug, Serialize)]
pub struct TypedDataDraft {
    pub ok: bool,
    pub id: String,
    pub typed_data: serde_json::Value,
}

pub async fn session_draft_typed_data(
    body: web::Json<SessionDraftRequest>,
) -> impl Responder {
    if !is_hex_addr(&body.owner)
        || !is_hex_addr(&body.verifying_contract)
        || !is_hex_addr(&body.policy.session_key)
    {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": "Bad address" }));
    }

    let targets: Vec<String> = body.policy.allowed_targets.iter().map(|t| lc(t)).collect();
    for t in &targets {
        if !is_hex_addr(t) {
            return HttpResponse::BadRequest().json(serde_json::json!({ "error": "Bad target address" }));
        }
    }

    let selectors: Vec<String> = body
        .policy
        .allowed_selectors
        .iter()
        .map(|s| {
            let t = s.trim();
            if t.starts_with("0x") { t.to_string() } else { format!("0x{}", t) }
        })
        .collect();

    let id = Uuid::new_v4().to_string();
    let deadline = (chrono::Utc::now().timestamp() as i64 + 3600).max(0) as u64;

    let typed = serde_json::json!({
      "types": {
        "EIP712Domain": [
          {"name":"name","type":"string"},
          {"name":"version","type":"string"},
          {"name":"chainId","type":"uint256"},
          {"name":"verifyingContract","type":"address"}
        ],
        "SessionPermit": [
          {"name":"sessionKey","type":"address"},
          {"name":"expiry","type":"uint64"},
          {"name":"maxValueWeiPerTx","type":"uint256"},
          {"name":"maxCallsPerMinute","type":"uint32"},
          {"name":"targets","type":"address[]"},
          {"name":"selectors","type":"bytes4[]"},
          {"name":"nonce","type":"uint256"},
          {"name":"deadline","type":"uint256"}
        ]
      },
      "primaryType":"SessionPermit",
      "domain":{
        "name":"CryptoCrowned Smart Wallet",
        "version":"1",
        "chainId": body.chain_id,
        "verifyingContract": lc(&body.verifying_contract)
      },
      "message":{
        "sessionKey": lc(&body.policy.session_key),
        "expiry": body.policy.expiry,
        "maxValueWeiPerTx": body.policy.max_value_wei_per_tx,
        "maxCallsPerMinute": body.policy.max_calls_per_min,
        "targets": targets,
        "selectors": selectors,
        "nonce": "0",
        "deadline": deadline.to_string()
      }
    });

    HttpResponse::Ok().json(TypedDataDraft { ok: true, id, typed_data: typed })
}

#[derive(Debug, Deserialize)]
pub struct SessionActivateRequest {
    pub owner: String,
    pub policy: SessionPolicy,
    pub sig: String,
}

pub async fn session_activate_handler(
    state: web::Data<WalletState>,
    body: web::Json<SessionActivateRequest>,
) -> impl Responder {
    if !is_hex_addr(&body.owner) || !is_hex_addr(&body.policy.session_key) {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": "Bad address" }));
    }
    if !body.sig.starts_with("0x") || body.sig.len() < 10 {
        return HttpResponse::BadRequest().json(serde_json::json!({ "error": "Bad signature" }));
    }
    let owner = lc(&body.owner);
    state.sessions.lock().unwrap().insert(owner, body.policy.clone());
    HttpResponse::Ok().json(serde_json::json!({
      "ok": true,
      "note": "Stored session policy server-side. Next: call CCSmartWallet.installSessionWithSig(...) on-chain using the signed typed data."
    }))
}
