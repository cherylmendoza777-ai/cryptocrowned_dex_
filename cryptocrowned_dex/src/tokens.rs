use actix_web::{HttpResponse, Responder};

pub async fn tokens() -> impl Responder {
    HttpResponse::Ok().json(vec![
        serde_json::json!({
            "symbol": "ETH",
            "name": "Ethereum",
            "chain": "ethereum",
            "price_usd": 3321.45,
            "change_24h_pct": 2.84,
            "volume_24h_usd": 18345678912u64
        }),
        serde_json::json!({
            "symbol": "USDC",
            "name": "USD Coin",
            "chain": "ethereum",
            "price_usd": 1.0,
            "change_24h_pct": 0.0,
            "volume_24h_usd": 9456123781u64
        }),
    ])
}
