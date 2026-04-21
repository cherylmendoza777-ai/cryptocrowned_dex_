// src/copy_trade.rs
use actix_web::{web, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

use crate::execute::{prepare_execute_internal, ExecutePrepareRequest};
use crate::risk::RiskState;

#[derive(Debug, Deserialize, Clone)]
pub struct FollowRequest {
    pub follower: String,
    pub leader: String,
    pub chain: String,
    pub copy_buys: bool,
    pub copy_sells: bool,
    pub sell_pct_on_sell: u32, // 0..100
    pub slippage_bps: u32,
    pub max_sell_amount_wei: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UnfollowRequest {
    pub follower: String,
    pub leader: String,
    pub chain: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SignalRequest {
    pub leader: String,
    pub chain: String,
    pub sell: String,
    pub buy: String,
    pub sell_amount: String,
    pub side: String, // "buy" | "sell"
}

#[derive(Debug, Serialize, Clone)]
pub struct FollowerConfig {
    pub follower: String,
    pub leader: String,
    pub chain: String,
    pub copy_buys: bool,
    pub copy_sells: bool,
    pub sell_pct_on_sell: u32,
    pub slippage_bps: u32,
    pub max_sell_amount_wei: String,
}

#[derive(Debug, Serialize)]
pub struct SignalFollowerResult {
    pub follower: String,
    pub ok: bool,
    pub error: Option<String>,
    pub prepared: Option<serde_json::Value>,
}

pub struct CopyState {
    inner: Arc<Mutex<HashMap<String, Vec<FollowerConfig>>>>, // key = leader|chain
}

impl CopyState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn key(leader: &str, chain: &str) -> String {
    format!("{}|{}", leader.to_lowercase(), chain.to_lowercase())
}

pub async fn follow_handler(
    state: web::Data<CopyState>,
    body: web::Json<FollowRequest>,
) -> impl Responder {
    let mut map = state.inner.lock().unwrap();
    let k = key(&body.leader, &body.chain);
    let v = map.entry(k).or_insert_with(Vec::new);

    // de-dupe by follower
    v.retain(|c| c.follower.to_lowercase() != body.follower.to_lowercase());

    v.push(FollowerConfig {
        follower: body.follower.clone(),
        leader: body.leader.clone(),
        chain: body.chain.clone(),
        copy_buys: body.copy_buys,
        copy_sells: body.copy_sells,
        sell_pct_on_sell: body.sell_pct_on_sell.min(100),
        slippage_bps: body.slippage_bps,
        max_sell_amount_wei: body.max_sell_amount_wei.clone(),
    });

    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

pub async fn unfollow_handler(
    state: web::Data<CopyState>,
    body: web::Json<UnfollowRequest>,
) -> impl Responder {
    let mut map = state.inner.lock().unwrap();
    let k = key(&body.leader, &body.chain);
    if let Some(v) = map.get_mut(&k) {
        v.retain(|c| c.follower.to_lowercase() != body.follower.to_lowercase());
    }
    HttpResponse::Ok().json(serde_json::json!({ "ok": true }))
}

pub async fn followers_handler(state: web::Data<CopyState>) -> impl Responder {
    let map = state.inner.lock().unwrap();
    let mut all: Vec<FollowerConfig> = vec![];
    for v in map.values() {
        all.extend(v.clone());
    }
    HttpResponse::Ok().json(all)
}

pub async fn signal_handler(
    copy_state: web::Data<CopyState>,
    risk_state: web::Data<RiskState>,
    body: web::Json<SignalRequest>,
) -> impl Responder {
    let side = body.side.trim().to_lowercase();
    if side != "buy" && side != "sell" {
        return HttpResponse::BadRequest()
            .json(serde_json::json!({ "error": "side must be buy or sell" }));
    }

    let map = copy_state.inner.lock().unwrap();
    let k = key(&body.leader, &body.chain);
    let followers = map.get(&k).cloned().unwrap_or_default();
    drop(map);

    let mut results: Vec<SignalFollowerResult> = vec![];

    for f in followers.iter() {
        // filters
        if side == "buy" && !f.copy_buys {
            continue;
        }
        if side == "sell" && !f.copy_sells {
            continue;
        }

        // optional: scale sells
        let mut sell_amount = body.sell_amount.clone();
        if side == "sell" && f.sell_pct_on_sell < 100 {
            if let Ok(a) = body.sell_amount.parse::<u128>() {
                let scaled = a.saturating_mul(f.sell_pct_on_sell as u128) / 100u128;
                sell_amount = scaled.to_string();
            }
        }

        let exec_req = ExecutePrepareRequest {
            chain: f.chain.clone(),
            sell: body.sell.clone(),
            buy: body.buy.clone(),
            sell_amount,
            slippage_bps: f.slippage_bps,
            recipient: f.follower.clone(),
        };

        // internal prepare with a fixed ip for now (server-side signal)
        let ip: IpAddr = "127.0.0.1".parse().unwrap();

        match prepare_execute_internal(&risk_state, ip, &exec_req).await {
            Ok(prepared) => results.push(SignalFollowerResult {
                follower: f.follower.clone(),
                ok: true,
                error: None,
                prepared: Some(serde_json::to_value(prepared).unwrap_or(serde_json::json!(null))),
            }),
            Err(e) => results.push(SignalFollowerResult {
                follower: f.follower.clone(),
                ok: false,
                error: Some(e),
                prepared: None,
            }),
        }
    }

    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "leader": body.leader,
        "chain": body.chain,
        "side": side,
        "attempted": results.len(),
        "results": results
    }))
}
