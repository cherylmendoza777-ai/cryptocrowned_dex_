use serde::Serialize;

const FEE_BPS: u128 = 25; // 0.25% = 25 bps
const FEE_WALLET: &str = "0x33b2a32ad5c5af72dd027b9dcec73de84b4f5393";

#[derive(Serialize)]
pub struct QuoteResponse {
    pub venue: String,

    // raw amounts (base units)
    pub sell_amount: String,
    pub buy_amount: String,

    // formatted amounts (human units)
    pub estimated_out: String, // gross_out formatted
    pub fee_out: String,       // fee formatted
    pub net_out: String,       // net formatted

    // raw fee fields (base units)
    pub fee_amount: String,
    pub net_amount: String,

    // fee meta
    pub fee_bps: u32,
    pub fee_wallet: String,
}

fn decimals(token: &str) -> u32 {
    match token.to_lowercase().as_str() {
        // Ethereum mainnet common tokens
        "0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48" => 6, // USDC
        "0xdac17f958d2ee523a2206206994597c13d831ec7" => 6, // USDT
        "0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2" => 18, // WETH
        "0x6b175474e89094c44da98b954eedeac495271d0f" => 18, // DAI
        _ => 18,
    }
}

fn format_units(amount: u128, dec: u32) -> String {
    let v = (amount as f64) / 10f64.powi(dec as i32);
    format!("{:.6}", v)
}

fn apply_fee(gross_out: u128) -> (u128, u128) {
    // fee = gross * 25 / 10000
    let fee = gross_out.saturating_mul(FEE_BPS) / 10_000u128;
    let net = gross_out.saturating_sub(fee);
    (fee, net)
}

pub async fn best_quote(
    sell_token: &str,
    buy_token: &str,
    amount: &str,
    zero_x_key: &str,
) -> Result<QuoteResponse, String> {
    let client = reqwest::Client::new();

    // -------------------------
    // 0x QUOTE (SAFE)
    // -------------------------
    let zero_x_out: Option<u128> = match client
        .get("https://api.0x.org/swap/v1/quote")
        .header("0x-api-key", zero_x_key)
        .query(&[
            ("sellToken", sell_token),
            ("buyToken", buy_token),
            ("sellAmount", amount),
        ])
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let body = resp.text().await.unwrap_or_default();
            let json =
                serde_json::from_str::<serde_json::Value>(&body).unwrap_or(serde_json::Value::Null);

            json.get("buyAmount")
                .and_then(|v| v.as_str())
                .and_then(|v| v.parse::<u128>().ok())
        }
        _ => None,
    };

    // -------------------------
    // 1INCH QUOTE (AUTH REQUIRED)
    // -------------------------
    let one_inch_key = std::env::var("ONEINCH_API_KEY").unwrap_or_default();

    let one_inch_out: Option<u128> = match client
        .get("https://api.1inch.dev/swap/v5.2/1/quote")
        .header("Authorization", format!("Bearer {}", one_inch_key))
        .query(&[
            ("fromTokenAddress", sell_token),
            ("toTokenAddress", buy_token),
            ("amount", amount),
        ])
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            let body = resp.text().await.unwrap_or_default();
            let json =
                serde_json::from_str::<serde_json::Value>(&body).unwrap_or(serde_json::Value::Null);

            json.get("toAmount")
                .and_then(|v| v.as_str())
                .and_then(|v| v.parse::<u128>().ok())
        }
        _ => None,
    };

    // -------------------------
    // PICK BEST
    // -------------------------
    let (venue, gross_out) = match (zero_x_out, one_inch_out) {
        (Some(z), Some(o)) if z >= o => ("0x", z),
        (Some(_), Some(o)) => ("1inch", o),
        (Some(z), None) => ("0x", z),
        (None, Some(o)) => ("1inch", o),
        _ => return Err("No valid route from 0x or 1inch".into()),
    };

    // -------------------------
    // APPLY FEE + FORMAT
    // -------------------------
    let out_dec = decimals(buy_token);
    let (fee_amt, net_amt) = apply_fee(gross_out);

    Ok(QuoteResponse {
        venue: venue.into(),
        sell_amount: amount.into(),

        buy_amount: gross_out.to_string(),
        estimated_out: format_units(gross_out, out_dec),

        fee_amount: fee_amt.to_string(),
        net_amount: net_amt.to_string(),

        fee_out: format_units(fee_amt, out_dec),
        net_out: format_units(net_amt, out_dec),

        fee_bps: FEE_BPS as u32,
        fee_wallet: FEE_WALLET.into(),
    })
}
