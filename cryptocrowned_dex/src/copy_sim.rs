use serde::Serialize;

const FEE_BPS: f64 = 0.25;

#[derive(Serialize)]
pub struct SimulatedTrade {
    pub trade_id: u32,
    pub pnl_pct: f64,
    pub win: bool,
}

#[derive(Serialize)]
pub struct CopySimulation {
    pub trader: String,
    pub days: u32,
    pub starting_capital: f64,
    pub ending_capital: f64,
    pub roi_pct: f64,
    pub winrate_pct: f64,
    pub trades: u32,
    pub total_fees_paid: f64,
    pub max_drawdown_pct: f64,
    pub trade_log: Vec<SimulatedTrade>,
}

pub fn simulate_copy_trade(trader: &str, amount: f64, days: u32) -> CopySimulation {
    // deterministic stub (safe + testable)
    let trades = if days <= 15 {
        24
    } else if days <= 30 {
        52
    } else {
        96
    };

    let mut capital = amount;
    let mut peak = amount;
    let mut max_dd = 0.0;
    let mut wins = 0;
    let mut fees_paid = 0.0;

    let mut log = vec![];

    for i in 0..trades {
        let pnl = if i % 4 == 0 { -0.9 } else { 1.8 }; // ~75% winrate
        let gross_change = capital * (pnl / 100.0);
        let fee = capital * (FEE_BPS / 100.0);

        capital += gross_change - fee;
        fees_paid += fee;

        if capital > peak {
            peak = capital;
        }

        let dd = ((peak - capital) / peak) * 100.0;
        if dd > max_dd {
            max_dd = dd;
        }

        if pnl > 0.0 {
            wins += 1;
        }

        log.push(SimulatedTrade {
            trade_id: i + 1,
            pnl_pct: pnl,
            win: pnl > 0.0,
        });
    }

    let roi = ((capital - amount) / amount) * 100.0;
    let winrate = (wins as f64 / trades as f64) * 100.0;

    CopySimulation {
        trader: trader.into(),
        days,
        starting_capital: amount,
        ending_capital: capital,
        roi_pct: roi,
        winrate_pct: winrate,
        trades,
        total_fees_paid: fees_paid,
        max_drawdown_pct: max_dd,
        trade_log: log,
    }
}
