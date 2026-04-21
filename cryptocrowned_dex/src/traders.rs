use actix_web::{HttpResponse, Responder};

pub async fn traders() -> impl Responder {
    HttpResponse::Ok().json(vec![
        serde_json::json!({
            "trader_id": "0xABC123",
            "chain": "ethereum",
            "roi_30d_pct": 245.7,
            "winrate_pct": 82.4,
            "trades_30d": 128,
            "followers": 421,
            "risk": "medium"
        }),
        serde_json::json!({
            "trader_id": "0xDEF456",
            "chain": "ethereum",
            "roi_30d_pct": 512.9,
            "winrate_pct": 76.1,
            "trades_30d": 302,
            "followers": 1337,
            "risk": "high"
        }),
    ])
}
