// Debt payoff planner. Simulates paying down liability accounts month by month
// under a snowball (smallest balance first) or avalanche (highest APR first)
// strategy, with an optional extra monthly payment that rolls forward as each
// debt clears. Offline — uses each liability account's latest known balance and
// its stored APR / minimum payment.

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::State;

#[derive(Debug, Serialize)]
pub struct LiabilityAccount {
    pub account_id: i64,
    pub name: String,
    pub balance: Option<f64>,
    pub apr: Option<f64>,
    pub min_payment: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct DebtPayoffLine {
    pub account_id: i64,
    pub name: String,
    pub starting_balance: f64,
    pub interest_paid: f64,
    /// 1-based month this debt is cleared, or null if not cleared in the horizon.
    pub payoff_month: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct DebtPayoffPlan {
    pub strategy: String,
    pub extra_payment: f64,
    pub months_to_debt_free: Option<i64>,
    pub total_interest: f64,
    pub total_paid: f64,
    pub starting_balance: f64,
    pub debts: Vec<DebtPayoffLine>,
    /// Total remaining balance at the end of each month, for charting.
    pub balance_trajectory: Vec<f64>,
    /// True if minimum payments + extra can't cover the interest (debt grows).
    pub underwater: bool,
}

/// Liability accounts with the inputs the planner needs. Surfaced so the UI can
/// prompt the user to fill in any missing APR / minimum payment / balance.
#[tauri::command]
pub async fn list_liabilities(pool: State<'_, SqlitePool>) -> Result<Vec<LiabilityAccount>, String> {
    sqlx::query_as::<_, (i64, String, Option<f64>, Option<f64>, Option<f64>)>(
        "SELECT a.id, a.name, a.apr, a.min_payment,
            (SELECT t.balance FROM transactions t
             WHERE t.account_id = a.id AND t.balance IS NOT NULL
             ORDER BY t.date DESC, t.id DESC LIMIT 1) AS balance
         FROM accounts a
         WHERE a.type = 'liability'
         ORDER BY a.name COLLATE NOCASE",
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))
    .map(|rows| {
        rows.into_iter()
            .map(|(account_id, name, apr, min_payment, balance)| LiabilityAccount {
                account_id,
                name,
                balance,
                apr,
                min_payment,
            })
            .collect()
    })
}

#[tauri::command]
pub async fn update_account_debt_terms(
    pool: State<'_, SqlitePool>,
    account_id: i64,
    apr: Option<f64>,
    min_payment: Option<f64>,
) -> Result<(), String> {
    sqlx::query("UPDATE accounts SET apr = ?, min_payment = ? WHERE id = ?")
        .bind(apr)
        .bind(min_payment)
        .bind(account_id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB update error: {}", e))?;
    Ok(())
}

struct SimDebt {
    account_id: i64,
    name: String,
    starting_balance: f64,
    balance: f64,
    monthly_rate: f64,
    min_payment: f64,
    interest_paid: f64,
    payoff_month: Option<i64>,
}

const MAX_MONTHS: i64 = 600; // 50 years — guards against an endless loop.

#[tauri::command]
pub async fn simulate_debt_payoff(
    pool: State<'_, SqlitePool>,
    extra_payment: Option<f64>,
    strategy: Option<String>,
) -> Result<DebtPayoffPlan, String> {
    let extra_payment = extra_payment.unwrap_or(0.0).max(0.0);
    let strategy = strategy.unwrap_or_else(|| "avalanche".to_string());

    let liabilities = list_liabilities(pool).await?;
    let mut debts: Vec<SimDebt> = liabilities
        .into_iter()
        .filter_map(|l| {
            let balance = l.balance?;
            if balance <= 0.0 {
                return None;
            }
            Some(SimDebt {
                account_id: l.account_id,
                name: l.name,
                starting_balance: balance,
                balance,
                monthly_rate: l.apr.unwrap_or(0.0) / 100.0 / 12.0,
                min_payment: l.min_payment.unwrap_or(0.0).max(0.0),
                interest_paid: 0.0,
                payoff_month: None,
            })
        })
        .collect();

    if debts.is_empty() {
        return Err("No liability accounts with a balance to simulate. Set a balance, APR and minimum payment first.".to_string());
    }

    let starting_balance: f64 = debts.iter().map(|d| d.balance).sum();

    // Payoff priority order (indices into `debts`).
    let mut order: Vec<usize> = (0..debts.len()).collect();
    match strategy.as_str() {
        "snowball" => order.sort_by(|&a, &b| {
            debts[a].balance.partial_cmp(&debts[b].balance).unwrap()
        }),
        // avalanche (default): highest rate first.
        _ => order.sort_by(|&a, &b| {
            debts[b].monthly_rate.partial_cmp(&debts[a].monthly_rate).unwrap()
        }),
    }

    let mut total_interest = 0.0;
    let mut total_paid = 0.0;
    let mut trajectory: Vec<f64> = Vec::new();
    let mut month = 0i64;
    let mut underwater = false;

    while debts.iter().any(|d| d.balance > 0.005) {
        month += 1;
        if month > MAX_MONTHS {
            underwater = true;
            break;
        }

        // 1. Accrue interest on every outstanding debt.
        for d in debts.iter_mut() {
            if d.balance > 0.0 {
                let interest = d.balance * d.monthly_rate;
                d.balance += interest;
                d.interest_paid += interest;
                total_interest += interest;
            }
        }

        // 2. This month's payment budget = sum of minimums + the extra, plus
        //    any minimums freed by already-cleared debts (rolled forward).
        let mut budget: f64 = extra_payment
            + debts.iter().filter(|d| d.balance > 0.0).map(|d| d.min_payment).sum::<f64>();

        // 3. Pay minimums first (so every debt at least services its minimum),
        //    then throw the remainder at the priority debt.
        for d in debts.iter_mut() {
            if d.balance > 0.0 {
                let pay = d.min_payment.min(d.balance);
                d.balance -= pay;
                budget -= pay;
                total_paid += pay;
            }
        }

        // 4. Funnel whatever is left to debts in priority order.
        for &i in &order {
            if budget <= 0.005 {
                break;
            }
            let d = &mut debts[i];
            if d.balance > 0.0 {
                let pay = budget.min(d.balance);
                d.balance -= pay;
                budget -= pay;
                total_paid += pay;
            }
        }

        // 5. Detect debts cleared this month + a stalled (growing) total.
        let mut remaining = 0.0;
        for d in debts.iter_mut() {
            if d.balance <= 0.005 {
                d.balance = 0.0;
                if d.payoff_month.is_none() {
                    d.payoff_month = Some(month);
                }
            }
            remaining += d.balance;
        }
        trajectory.push(remaining);

        // If no progress is being made (payments don't beat interest), bail.
        if trajectory.len() >= 2 {
            let prev = trajectory[trajectory.len() - 2];
            if remaining >= prev - 0.005 && remaining > 0.0 {
                underwater = true;
                break;
            }
        }
    }

    let cleared = debts.iter().all(|d| d.balance <= 0.005);
    let months_to_debt_free = if cleared && !underwater { Some(month) } else { None };

    Ok(DebtPayoffPlan {
        strategy,
        extra_payment,
        months_to_debt_free,
        total_interest,
        total_paid,
        starting_balance,
        debts: debts
            .iter()
            .map(|d| DebtPayoffLine {
                account_id: d.account_id,
                name: d.name.clone(),
                starting_balance: d.starting_balance,
                interest_paid: d.interest_paid,
                payoff_month: d.payoff_month,
            })
            .collect(),
        balance_trajectory: trajectory,
        underwater,
    })
}
