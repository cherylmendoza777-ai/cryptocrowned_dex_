// src/main.rs

use actix::prelude::*;
use actix_cors::Cors;
use actix_files::Files;
use actix_web::{
    middleware::Logger, web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder,
};
use actix_web_actors::ws;
use log::info;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

// =============== MODULES ===============
mod copy_sim;
mod copy_trade;
mod erc20;
mod execute;
mod quote;
mod risk;
mod tokens;
mod traders;
mod wallet;

// =============== IMPORTS ===============
use copy_sim::simulate_copy_trade;
use copy_trade::{
    CopyState, follow_handler, unfollow_handler, followers_handler, signal_handler,
};
use erc20::{allowance_handler, approve_prepare_handler};
use execute::prepare_execute;
use quote::best_quote;
use risk::{risk_check_handler, RiskState};
use tokens::tokens;
use traders::traders;
use wallet::{
    WalletState, wallet_register_handler, session_draft_typed_data, session_activate_handler,
};

// =============== CONFIG ===============
const FEE_WALLET: &str = "0x33b2a32ad5c5af72dd027b9dcec73de84b4f5393";
const FEE_BPS: u32 = 25;

#[derive(Serialize)]
struct AppConfig {
    fee_bps: u32,
    fee_wallet: String,
    supported_chains: Vec<String>,
    mode: String,
}

// =============== QUOTE ENDPOINT ===============
async fn quote_handler(q: web::Query<HashMap<String, String>>) -> impl Responder {
    let sell = q.get("sell").map(String::as_str).unwrap_or("");
    let buy  = q.get("buy").map(String::as_str).unwrap_or("");
    let amount = q.get("amount").map(String::as_str).unwrap_or("");

    if sell.is_empty() || buy.is_empty() || amount.is_empty() {
        return HttpResponse::BadRequest().body("Missing sell, buy, or amount");
    }

    let zero_x_key = std::env::var("ZEROX_API_KEY").unwrap_or_default();
    match best_quote(sell, buy, amount, &zero_x_key).await {
        Ok(res) => HttpResponse::Ok().json(res),
        Err(e)  => HttpResponse::BadRequest().body(e),
    }
}

// =============== COPY SIMULATION ===============
async fn copy_sim_handler(q: web::Query<HashMap<String, String>>) -> impl Responder {
    let trader = q.get("trader").cloned().unwrap_or_else(|| "unknown".into());
    let amount: f64 = q.get("amount").and_then(|v| v.parse().ok()).unwrap_or(1000.0);
    let days: u32  = q.get("days").and_then(|v| v.parse().ok()).unwrap_or(30);

    let sim = simulate_copy_trade(&trader, amount, days);
    HttpResponse::Ok().json(sim)
}

// =============== HEALTH / CONFIG ===============
async fn health() -> impl Responder {
    HttpResponse::Ok().json(serde_json::json!({
        "ok": true,
        "service": "cryptocrowned_dex"
    }))
}

async fn config() -> impl Responder {
    HttpResponse::Ok().json(AppConfig {
        fee_bps: FEE_BPS,
        fee_wallet: FEE_WALLET.into(),
        supported_chains: vec!["ethereum".into()],
        mode: "live".into(),
    })
}

// =============== WEBSOCKET SESSION ===============
struct WsSession {
    id: String,
}

impl Actor for WsSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.run_interval(Duration::from_secs(2), |act, ctx| {
            ctx.text(
                serde_json::json!({
                    "type": "tick",
                    "session": act.id,
                    "ts": now_ms()
                })
                .to_string(),
            );
        });
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsSession {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        if let Ok(ws::Message::Text(text)) = msg {
            ctx.text(
                serde_json::json!({
                    "type": "echo",
                    "session": self.id,
                    "received": text.to_string()
                })
                .to_string(),
            );
        }
    }
}

async fn ws_handler(req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    ws::start(
        WsSession {
            id: Uuid::new_v4().to_string(),
        },
        &req,
        stream,
    )
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// =============== MAIN ===============
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse()
        .unwrap();

    // Shared state for risk engine, copy-trade configs, and wallet sessions
    let risk_state  = web::Data::new(RiskState::from_env());
    let copy_state  = web::Data::new(CopyState::new());
    let wallet_state = web::Data::new(WalletState::new());

    info!("CryptoCrowned DEX running on 0.0.0.0:{port}");

    HttpServer::new(move || {
        App::new()
            // inject state
            .app_data(risk_state.clone())
            .app_data(copy_state.clone())
            .app_data(wallet_state.clone())
            // middleware
            .wrap(Logger::default())
            .wrap(Cors::permissive())
            // Core endpoints
            .route("/api/health", web::get().to(health))
            .route("/api/config", web::get().to(config))
            .route("/api/quote", web::get().to(quote_handler))
            .route("/api/tokens", web::get().to(tokens))
            .route("/api/traders", web::get().to(traders))
            .route("/api/copy/simulate", web::get().to(copy_sim_handler))
            // Risk + execution
            .route("/api/risk/check", web::post().to(risk_check_handler))
            .route("/api/execute/prepare", web::post().to(prepare_execute))
            // ERC-20 allowance / approval (Step B)
            .route("/api/erc20/allowance", web::post().to(allowance_handler))
            .route("/api/erc20/approve/prepare", web::post().to(approve_prepare_handler))
            // Copy-trading APIs
            .route("/api/copy/follow", web::post().to(follow_handler))
            .route("/api/copy/unfollow", web::post().to(unfollow_handler))
            .route("/api/copy/followers", web::get().to(followers_handler))
            .route("/api/copy/signal", web::post().to(signal_handler))
            // Smart-wallet session endpoints (Step E)
            .route("/api/wallet/register", web::post().to(wallet_register_handler))
            .route("/api/wallet/session/draft", web::post().to(session_draft_typed_data))
            .route("/api/wallet/session/activate", web::post().to(session_activate_handler))
            // WebSocket + static files
            .route("/ws", web::get().to(ws_handler))
            .service(Files::new("/", "./static").index_file("index.html"))
    })
    .bind(("0.0.0.0", port))?
    .run()
    .await
}
