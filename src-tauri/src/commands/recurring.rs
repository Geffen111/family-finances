// Detects recurring charges (subscriptions, regular bills) by grouping
// expense transactions by merchant and looking for a regular cadence.

use crate::commands::categorise::normalize_desc;
use crate::models::RecurringCost;
use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::{HashMap, HashSet};
use tauri::State;

/// The cadence for something that happens once — school camp, a rego bill you
/// are not sure will repeat. Stored in the same `frequency` column as the
/// repeating cadences, so there is one list to read and one list to forecast.
pub const ONE_OFF: &str = "One-off";

/// Which way the money goes. Stored on the same row as the cadence, because a
/// fortnightly pay and a fortnightly bill differ only in sign — splitting them
/// into two tables would duplicate the expansion and the forecast wiring.
pub const KIND_EXPENSE: &str = "expense";
pub const KIND_INCOME: &str = "income";

/// An unknown kind is an error rather than a silent "expense": a mistyped kind
/// would otherwise quietly subtract your salary.
pub fn validate_kind(kind: &str) -> Result<(), String> {
    if kind == KIND_EXPENSE || kind == KIND_INCOME {
        Ok(())
    } else {
        Err(format!(
            "Unknown kind \"{}\" — expected \"expense\" or \"income\".",
            kind
        ))
    }
}

/// Canonical payment frequencies and how many payments they amount to per
/// calendar month. Used to normalise a recurring cost to a monthly figure.
/// Keep the labels in sync with the frontend `<select>` options.
pub fn payments_per_month(frequency: &str) -> f64 {
    match frequency {
        // A one-off has no monthly equivalent. Zero, not the 1.0 fallback: it
        // must not inflate the "per month" total, and the pages that care show
        // its dated amount instead.
        ONE_OFF => 0.0,
        "Weekly" => 52.0 / 12.0,
        "Fortnightly" => 26.0 / 12.0,
        "Monthly" => 1.0,
        "Every 2 months" => 0.5,
        "Quarterly" => 1.0 / 3.0,
        "Half-yearly" => 1.0 / 6.0,
        "Yearly" => 1.0 / 12.0,
        // Unknown cadence: treat as monthly rather than zeroing the cost.
        _ => 1.0,
    }
}

#[derive(Debug, Serialize)]
pub struct RecurringItem {
    pub description: String,
    pub category: String,
    /// The id as well as the name, so adding a detected item to the tracked
    /// list can carry its category over instead of matching on text.
    pub category_id: Option<i64>,
    /// True when this is money coming in (a pay, a dividend).
    pub is_income: bool,
    pub frequency: String,
    pub occurrences: i64,
    pub avg_amount: f64,
    pub monthly_cost: f64,
    pub last_date: String,
    /// `last_date` advanced by one cycle, rolled forward if that is still in
    /// the past — what the tracked item's next due date should be.
    pub next_due_date: String,
    /// True when an upcoming expense with the same merchant key is already
    /// tracked, so the UI can offer the rest and not re-add this one.
    pub already_tracked: bool,
    /// True when this was found by amount rather than by merchant alone —
    /// the same figure on the same rhythm, under a reference that changes
    /// every month. Worth saying, because it is the weaker signal.
    pub fixed_amount: bool,
}

fn classify(median_gap: f64) -> Option<(&'static str, f64)> {
    // Returns (label, payments-per-month) for a plausible cadence, else None.
    let per_month = 30.44 / median_gap;
    let label = match median_gap {
        g if (5.0..=10.0).contains(&g) => "Weekly",
        g if (11.0..=18.0).contains(&g) => "Fortnightly",
        g if (19.0..=45.0).contains(&g) => "Monthly",
        g if (46.0..=75.0).contains(&g) => "Every 2 months",
        g if (76.0..=135.0).contains(&g) => "Quarterly",
        g if (136.0..=225.0).contains(&g) => "Half-yearly",
        g if (300.0..=420.0).contains(&g) => "Yearly",
        _ => return None,
    };
    Some((label, per_month))
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = values.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

/// Category names that are pay-as-you-go spending rather than a commitment
/// anyone would put on the Upcoming list: a weekly supermarket run and a
/// Friday tank of fuel are regular, but they are not bills, and offering
/// them buries the handful of things that are.
///
/// Matched as whole words against the category's "Parent Child" path, so
/// naming a parent covers its whole subtree. Deliberately *not* words like
/// "entertainment", "online", "gaming" or "apps": that is exactly where
/// subscriptions live. The scan has a switch to include these anyway.
const EVERYDAY_WORDS: &[&str] = &[
    // eating and drinking
    "food", "grocery", "groceries", "supermarket", "dining", "cafe", "cafes",
    "café", "cafés", "restaurant", "restaurants", "takeaway", "coffee",
    "alcohol", "liquor", "pub", "bar",
    // shopping
    "retail", "clothes", "clothing", "shoes", "footwear", "apparel", "beauty",
    "cosmetics", "gift", "gifts", "toy", "toys", "homeware", "homewares",
    "furniture", "stationery", "hardware",
    // spending money, cash, and going away
    "discretionary", "withdrawal", "withdrawals", "atm", "cash", "travel",
    "holiday", "holidays",
    // getting about
    "fuel", "petrol", "parking",
    // health and pets: the regular ones here are visits, not bills
    "pharmacy", "chemist", "vet", "pet", "pets",
];

fn is_everyday(path: &str) -> bool {
    path.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|tok| EVERYDAY_WORDS.contains(&tok))
}

/// How long a series can have been quiet before it stops being worth
/// offering. A subscription cancelled last year is history, not a plan.
const STALE_AFTER_DAYS: i64 = 365;

struct Cadence {
    frequency: &'static str,
    per_month: f64,
}

/// Sorted dates in, cadence out. None when there are too few to judge, or
/// when the rhythm doesn't match a cadence we can name.
fn cadence_of(dates: &[NaiveDate]) -> Option<Cadence> {
    if dates.len() < 3 {
        return None;
    }
    let gaps: Vec<f64> = dates
        .windows(2)
        .map(|w| (w[1] - w[0]).num_days() as f64)
        .filter(|d| *d > 0.0)
        .collect();
    if gaps.is_empty() {
        return None;
    }
    let (frequency, per_month) = classify(median(gaps))?;
    Some(Cadence {
        frequency,
        per_month,
    })
}

fn cents(amount: f64) -> i64 {
    (amount * 100.0).round() as i64
}

/// How far apart two charges can be and still count as the same amount.
/// A pay that drifts a few dollars between periods is one thing, not several;
/// a $9.99 subscription and a $24.50 one are not.
const SAME_AMOUNT_TOLERANCE: f64 = 0.05;

/// Split a merchant's charges into clusters of roughly equal amounts, lowest
/// first. One cluster means the key is one thing and the amounts are just
/// noise around it; several means the key is covering several things.
fn cluster_by_amount(points: &[(NaiveDate, f64)]) -> Vec<Vec<(NaiveDate, f64)>> {
    let mut sorted: Vec<(NaiveDate, f64)> = points.to_vec();
    sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut clusters: Vec<Vec<(NaiveDate, f64)>> = Vec::new();
    for point in sorted {
        match clusters.last_mut() {
            Some(c) if point.1 <= c[0].1 * (1.0 + SAME_AMOUNT_TOLERANCE) => c.push(point),
            _ => clusters.push(vec![point]),
        }
    }
    clusters
}

#[tauri::command]
pub async fn get_recurring_transactions(
    pool: State<'_, SqlitePool>,
    include_everyday: Option<bool>,
) -> Result<Vec<RecurringItem>, String> {
    detect_recurring(&pool, include_everyday.unwrap_or(false)).await
}

/// Plain pool, so the detection rules are testable without a Tauri `State`.
pub(crate) async fn detect_recurring(
    pool: &SqlitePool,
    include_everyday: bool,
) -> Result<Vec<RecurringItem>, String> {
    let rows = sqlx::query_as::<_, (String, String, f64, f64, Option<String>, Option<i64>, Option<String>)>(
        "SELECT t.date, t.description, t.debit, t.credit, c.name, c.id, pc.name \
         FROM transactions t \
         LEFT JOIN categories c ON t.category_id = c.id \
         LEFT JOIN categories pc ON c.parent_id = pc.id \
         WHERE (t.debit > 0 OR t.credit > 0) \
           AND NOT EXISTS (SELECT 1 FROM categories xc WHERE xc.id = t.category_id AND xc.exclude_from_budget = 1) \
         ORDER BY t.date ASC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    struct Entry {
        /// Date and amount together: the second pass splits one merchant key
        /// by amount, so the two can't be kept in separate lists.
        points: Vec<(NaiveDate, f64)>,
        sample_desc: String,
        category: Option<String>,
        category_id: Option<i64>,
    }
    // Keyed by merchant *and* side: a merchant you both pay and get refunded by
    // is two different rhythms, and merging them would average them together.
    let mut groups: HashMap<(String, bool), Entry> = HashMap::new();

    for (date_str, desc, debit, credit, category, category_id, parent) in rows {
        if !include_everyday {
            let path = format!(
                "{} {}",
                parent.as_deref().unwrap_or(""),
                category.as_deref().unwrap_or("")
            );
            if is_everyday(&path) {
                continue;
            }
        }
        let key = normalize_desc(&desc);
        if key.is_empty() {
            continue;
        }
        let Ok(date) = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d") else {
            continue;
        };
        let is_income = credit > 0.0;
        let amount = if is_income { credit } else { debit };
        let e = groups.entry((key, is_income)).or_insert_with(|| Entry {
            points: Vec::new(),
            sample_desc: desc.clone(),
            category,
            category_id,
        });
        e.points.push((date, amount));
    }

    // What is already tracked, so a detected item that is already an upcoming
    // cost can be marked rather than offered again. By merchant key for a
    // whole-key candidate, and by key *and* amount for an amount-split one —
    // otherwise tracking the $9.99 subscription would hide the $24.50 one
    // behind the same merchant key.
    let tracked_rows =
        sqlx::query_as::<_, (String, String, f64)>("SELECT name, kind, amount FROM recurring_costs")
            .fetch_all(pool)
            .await
            .map_err(|e| format!("DB query error: {}", e))?;
    let tracked: HashSet<(String, bool)> = tracked_rows
        .iter()
        .map(|(n, kind, _)| (normalize_desc(n), kind == KIND_INCOME))
        .filter(|(k, _)| !k.is_empty())
        .collect();
    let tracked_at_amount: HashSet<(String, bool, i64)> = tracked_rows
        .iter()
        .map(|(n, kind, amount)| (normalize_desc(n), kind == KIND_INCOME, cents(*amount)))
        .filter(|(k, _, _)| !k.is_empty())
        .collect();

    let today = chrono::Local::now().naive_local().date();
    let stale_before = today - chrono::Duration::days(STALE_AFTER_DAYS);
    let mut items: Vec<RecurringItem> = Vec::new();

    for ((key, is_income), entry) in groups.into_iter() {
        let mut points = entry.points;
        points.sort_by_key(|(d, _)| *d);
        let dates: Vec<NaiveDate> = points.iter().map(|(d, _)| *d).collect();
        let description = entry
            .sample_desc
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let category = entry.category.unwrap_or_else(|| "Uncategorised".to_string());

        // One merchant key can cover several things, because some banks write
        // a different reference every time ("Wallet Co Australia 1234567890123")
        // and the key is all that survives. Then the *amount* is what tells
        // them apart: each cluster of roughly equal charges that keeps its own
        // rhythm is offered as its own candidate.
        //
        // Clustering with a tolerance, not grouping by the exact cents, is
        // what keeps a pay that drifts between periods as one thing instead
        // of three near-identical ones.
        let clusters = cluster_by_amount(&points);
        let mut fixed: Vec<(f64, Vec<NaiveDate>, Cadence)> = Vec::new();
        if clusters.len() > 1 {
            for cluster in &clusters {
                let mut ds: Vec<NaiveDate> = cluster.iter().map(|(d, _)| *d).collect();
                ds.sort();
                if let Some(cad) = cadence_of(&ds) {
                    let amount = median(cluster.iter().map(|(_, a)| *a).collect());
                    fixed.push((amount, ds, cad));
                }
            }
            fixed.sort_by_key(|(a, _, _)| cents(*a));
        }

        // Once the split has found the real rhythms inside a key, the average
        // over the whole lot is noise that would bury them — a key covering a
        // $9.99 subscription and a $90 game is not itself a cost. Where the
        // split found nothing, the key stands as it always did.
        let label_amounts = fixed.len() > 1;

        if fixed.is_empty() {
            if let Some(cad) = cadence_of(&dates) {
                let amounts: Vec<f64> = points.iter().map(|(_, a)| *a).collect();
                let avg = amounts.iter().sum::<f64>() / amounts.len() as f64;
                if let Some(item) = build_item(
                    description.clone(),
                    category.clone(),
                    entry.category_id,
                    is_income,
                    &cad,
                    &dates,
                    avg,
                    false,
                    tracked.contains(&(key.clone(), is_income)),
                    today,
                    stale_before,
                ) {
                    items.push(item);
                }
            }
        }

        for (amount, ds, cad) in &fixed {
            // Two subscriptions behind one merchant key would otherwise both
            // be offered under the same name; the amount is what you would
            // write on the card yourself.
            let name = if label_amounts {
                format!("{} ${:.2}", description, amount)
            } else {
                description.clone()
            };
            if let Some(item) = build_item(
                name,
                category.clone(),
                entry.category_id,
                is_income,
                cad,
                ds,
                *amount,
                true,
                tracked_at_amount.contains(&(key.clone(), is_income, cents(*amount))),
                today,
                stale_before,
            ) {
                items.push(item);
            }
        }
    }

    items.sort_by(|a, b| b.monthly_cost.partial_cmp(&a.monthly_cost).unwrap());
    Ok(items)
}

/// None when the series has gone quiet for longer than `stale_before` — a
/// cancelled subscription is history, and offering it is noise.
#[allow(clippy::too_many_arguments)]
fn build_item(
    description: String,
    category: String,
    category_id: Option<i64>,
    is_income: bool,
    cadence: &Cadence,
    dates: &[NaiveDate],
    amount: f64,
    fixed_amount: bool,
    already_tracked: bool,
    today: NaiveDate,
    stale_before: NaiveDate,
) -> Option<RecurringItem> {
    let last = *dates.last()?;
    if last < stale_before {
        return None;
    }

    // Where the next one falls: one cycle on from the last sighting, rolled
    // forward if the series has been quiet for longer than that. Reuses the
    // cadence arithmetic the bill projection already does.
    let mut next_due = crate::commands::cashflow::advance(last, cadence.frequency);
    let mut guard = 0;
    while next_due < today && guard < 1000 {
        next_due = crate::commands::cashflow::advance(next_due, cadence.frequency);
        guard += 1;
    }

    Some(RecurringItem {
        description,
        category,
        category_id,
        is_income,
        frequency: cadence.frequency.to_string(),
        occurrences: dates.len() as i64,
        avg_amount: amount,
        monthly_cost: amount * cadence.per_month,
        last_date: last.format("%Y-%m-%d").to_string(),
        next_due_date: next_due.format("%Y-%m-%d").to_string(),
        already_tracked,
        fixed_amount,
    })
}

// -- Manually managed recurring costs / subscriptions (CRUD) --

type RecurringRow = (
    i64,            // id
    String,         // name
    f64,            // amount
    String,         // frequency
    String,         // kind
    Option<i64>,    // category_id
    Option<String>, // category_name
    Option<String>, // next_due_date
    Option<String>, // end_date
    bool,           // active
    Option<String>, // notes
    String,         // created_at
);

/// Roll a stale due date forward to the next one that hasn't happened yet.
/// A one-off is never rolled — a past one-off is history, not a future bill —
/// and neither is anything that would land after the cost has ended.
fn next_occurrence_of(
    next_due_date: &Option<String>,
    frequency: &str,
    end_date: &Option<String>,
) -> Option<String> {
    let stored = NaiveDate::parse_from_str(next_due_date.as_deref()?, "%Y-%m-%d").ok()?;
    if frequency == ONE_OFF {
        return Some(stored.format("%Y-%m-%d").to_string());
    }
    let today = chrono::Local::now().naive_local().date();
    let mut next = stored;
    let mut guard = 0;
    while next < today && guard < 1000 {
        next = crate::commands::cashflow::advance(next, frequency);
        guard += 1;
    }
    if let Some(end) = end_date
        .as_deref()
        .and_then(|e| NaiveDate::parse_from_str(e, "%Y-%m-%d").ok())
    {
        if next > end {
            return Some(stored.format("%Y-%m-%d").to_string());
        }
    }
    Some(next.format("%Y-%m-%d").to_string())
}

fn row_to_cost(r: RecurringRow) -> RecurringCost {
    let (id, name, amount, frequency, kind, category_id, category_name, next_due_date, end_date, active, notes, created_at) = r;
    let monthly_cost = amount * payments_per_month(&frequency);
    let next_occurrence = next_occurrence_of(&next_due_date, &frequency, &end_date);
    RecurringCost {
        id,
        name,
        amount,
        frequency,
        kind,
        category_id,
        category_name,
        next_due_date,
        next_occurrence,
        end_date,
        active,
        notes,
        created_at,
        monthly_cost,
    }
}

const SELECT_RECURRING: &str = "SELECT r.id, r.name, r.amount, r.frequency, r.kind, r.category_id, c.name, \
     r.next_due_date, r.end_date, r.active, r.notes, r.created_at \
     FROM recurring_costs r \
     LEFT JOIN categories c ON r.category_id = c.id";

/// A date must parse, and an end date must not sit before the next due date —
/// a cost that ends before it starts would silently never appear anywhere.
/// A one-off without a date can't be placed at all, so it is refused here
/// rather than quietly dropped by the projection.
fn validate_dates(
    frequency: &str,
    next_due_date: &Option<String>,
    end_date: &Option<String>,
) -> Result<(), String> {
    let parse = |d: &str, what: &str| {
        NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| format!("{} must be a date in YYYY-MM-DD form, got \"{}\"", what, d))
    };
    let due = match next_due_date.as_deref().filter(|d| !d.is_empty()) {
        Some(d) => Some(parse(d, "The due date")?),
        None => {
            if frequency == ONE_OFF {
                return Err("A one-off needs a date — that is the only thing that places it.".into());
            }
            None
        }
    };
    if let Some(end) = end_date.as_deref().filter(|d| !d.is_empty()) {
        let end = parse(end, "The end date")?;
        if let Some(due) = due {
            if end < due {
                return Err(format!(
                    "The end date ({}) is before the next due date ({}), so this would never fall due.",
                    end, due
                ));
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn list_recurring_costs(pool: State<'_, SqlitePool>) -> Result<Vec<RecurringCost>, String> {
    let rows = sqlx::query_as::<_, RecurringRow>(
        &format!("{SELECT_RECURRING} ORDER BY r.active DESC, r.name COLLATE NOCASE"),
    )
    .fetch_all(&*pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    Ok(rows.into_iter().map(row_to_cost).collect())
}

/// Tracked items that look like one thing entered more than once.
#[derive(Debug, Serialize)]
pub struct DuplicateGroup {
    /// The first copy's name, for the heading — the others normalise to the
    /// same merchant key but may be worded differently.
    pub name: String,
    pub kind: String,
    pub frequency: String,
    pub amount: f64,
    pub items: Vec<RecurringCost>,
}

/// Group the live items that look like the same thing: same merchant key,
/// same side, same cadence, same amount. Dates are allowed to differ, which
/// is the point — the pair this mostly finds was added twice and only one
/// copy has since rolled forward to its next date.
///
/// Looser than `refuse_duplicate`, which has to be exact because it blocks a
/// save; this only reports, and the user says which copy to keep. Still
/// tighter than the merchant key alone: two loans both reading "Loan
/// Repayment" are separate until they also agree on what they cost.
pub(crate) async fn duplicate_groups(pool: &SqlitePool) -> Result<Vec<DuplicateGroup>, String> {
    let rows = sqlx::query_as::<_, RecurringRow>(&format!(
        "{SELECT_RECURRING} WHERE r.active = 1 ORDER BY r.id"
    ))
    .fetch_all(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    // Insertion-ordered, so the list reads in the order things were added
    // rather than in whatever order a hash map happens to hold them.
    let mut keys: Vec<String> = Vec::new();
    let mut groups: Vec<DuplicateGroup> = Vec::new();

    for cost in rows.into_iter().map(row_to_cost) {
        let key = format!(
            "{}|{}|{}|{:.2}",
            normalize_desc(&cost.name),
            cost.kind,
            cost.frequency,
            cost.amount
        );
        match keys.iter().position(|k| *k == key) {
            Some(i) => groups[i].items.push(cost),
            None => {
                keys.push(key);
                groups.push(DuplicateGroup {
                    name: cost.name.clone(),
                    kind: cost.kind.clone(),
                    frequency: cost.frequency.clone(),
                    amount: cost.amount,
                    items: vec![cost],
                });
            }
        }
    }

    Ok(groups.into_iter().filter(|g| g.items.len() > 1).collect())
}

#[tauri::command]
pub async fn find_duplicate_costs(
    pool: State<'_, SqlitePool>,
) -> Result<Vec<DuplicateGroup>, String> {
    duplicate_groups(&pool).await
}

/// Refuse a row that matches a live one on every field that identifies it:
/// name, amount, cadence, side and date. Two rows that agree on all five are
/// never two real things — they are one thing added twice, which is what a
/// double click, or an "Add" that errored after it had in fact written the
/// row, leaves behind.
///
/// Deliberately an *exact* match rather than a merchant-key one: two loans
/// both called "Loan Repayment" normalise to the same key but differ in
/// amount and date, and refusing the second would be wrong.
async fn refuse_duplicate(
    pool: &SqlitePool,
    name: &str,
    amount: f64,
    frequency: &str,
    kind: &str,
    next_due_date: &Option<String>,
) -> Result<(), String> {
    let clash = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM recurring_costs \
         WHERE active = 1 AND name = ? COLLATE NOCASE AND kind = ? AND frequency = ? \
           AND ABS(amount - ?) < 0.005 \
           AND IFNULL(next_due_date, '') = IFNULL(?, '')",
    )
    .bind(name)
    .bind(kind)
    .bind(frequency)
    .bind(amount)
    .bind(next_due_date)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("DB query error: {}", e))?;

    if clash > 0 {
        return Err(format!(
            "\"{}\" is already in the list at the same amount, cadence and date. \
             Edit that one, or change something here if this really is a second item.",
            name
        ));
    }
    Ok(())
}

/// Insert a recurring cost and read it back. Plain pool, like
/// `cashflow::expand_bills`, so it is testable without a Tauri `State`.
pub(crate) async fn insert_recurring_cost(
    pool: &SqlitePool,
    name: &str,
    amount: f64,
    frequency: &str,
    kind: Option<String>,
    category_id: Option<i64>,
    next_due_date: Option<String>,
    end_date: Option<String>,
    active: Option<bool>,
    notes: Option<String>,
) -> Result<RecurringCost, String> {
    let active = active.unwrap_or(true);
    let kind = kind.unwrap_or_else(|| KIND_EXPENSE.to_string());
    validate_kind(&kind)?;
    validate_dates(frequency, &next_due_date, &end_date)?;
    refuse_duplicate(pool, name, amount, frequency, &kind, &next_due_date).await?;
    // The id comes back from the INSERT itself. A follow-up
    // `SELECT last_insert_rowid()` is not safe here: the pool holds four
    // connections and hands out whichever is free, so that second statement
    // can run on a connection that has inserted nothing and answer 0.
    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO recurring_costs (name, amount, frequency, kind, category_id, next_due_date, end_date, active, notes) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(name)
    .bind(amount)
    .bind(frequency)
    .bind(&kind)
    .bind(category_id)
    .bind(&next_due_date)
    .bind(&end_date)
    .bind(active)
    .bind(&notes)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("DB insert error: {}", e))?;

    let row = sqlx::query_as::<_, RecurringRow>(&format!("{SELECT_RECURRING} WHERE r.id = ?"))
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    Ok(row_to_cost(row))
}

#[tauri::command]
pub async fn create_recurring_cost(
    pool: State<'_, SqlitePool>,
    name: String,
    amount: f64,
    frequency: String,
    kind: Option<String>,
    category_id: Option<i64>,
    next_due_date: Option<String>,
    end_date: Option<String>,
    active: Option<bool>,
    notes: Option<String>,
) -> Result<RecurringCost, String> {
    insert_recurring_cost(
        &pool,
        &name,
        amount,
        &frequency,
        kind,
        category_id,
        next_due_date,
        end_date,
        active,
        notes,
    )
    .await
}

#[tauri::command]
pub async fn update_recurring_cost(
    pool: State<'_, SqlitePool>,
    id: i64,
    name: String,
    amount: f64,
    frequency: String,
    kind: Option<String>,
    category_id: Option<i64>,
    next_due_date: Option<String>,
    end_date: Option<String>,
    active: Option<bool>,
    notes: Option<String>,
) -> Result<RecurringCost, String> {
    let active = active.unwrap_or(true);
    let kind = kind.unwrap_or_else(|| KIND_EXPENSE.to_string());
    validate_kind(&kind)?;
    validate_dates(&frequency, &next_due_date, &end_date)?;
    let affected = sqlx::query(
        "UPDATE recurring_costs \
         SET name = ?, amount = ?, frequency = ?, kind = ?, category_id = ?, next_due_date = ?, \
             end_date = ?, active = ?, notes = ? \
         WHERE id = ?",
    )
    .bind(&name)
    .bind(amount)
    .bind(&frequency)
    .bind(&kind)
    .bind(category_id)
    .bind(&next_due_date)
    .bind(&end_date)
    .bind(active)
    .bind(&notes)
    .bind(id)
    .execute(&*pool)
    .await
    .map_err(|e| format!("DB update error: {}", e))?
    .rows_affected();

    if affected == 0 {
        return Err(format!("Recurring cost {} not found", id));
    }

    let row = sqlx::query_as::<_, RecurringRow>(&format!("{SELECT_RECURRING} WHERE r.id = ?"))
        .bind(id)
        .fetch_one(&*pool)
        .await
        .map_err(|e| format!("DB query error: {}", e))?;

    Ok(row_to_cost(row))
}

#[tauri::command]
pub async fn delete_recurring_cost(pool: State<'_, SqlitePool>, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM recurring_costs WHERE id = ?")
        .bind(id)
        .execute(&*pool)
        .await
        .map_err(|e| format!("DB delete error: {}", e))?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// A pool with more than one live connection, over a shared in-memory
    /// database — the app's pool holds four. `sqlite::memory:` on its own
    /// gives each connection a *separate* database, which would hide exactly
    /// the cross-connection bug this is here to catch.
    async fn multi_connection_pool(name: &str) -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .min_connections(2)
            .max_connections(2)
            .connect(&format!("sqlite:file:{name}?mode=memory&cache=shared"))
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }

    async fn add(pool: &SqlitePool, name: &str, amount: f64, due: &str) -> Result<RecurringCost, String> {
        insert_recurring_cost(
            pool,
            name,
            amount,
            "Monthly",
            Some(KIND_EXPENSE.to_string()),
            None,
            Some(due.to_string()),
            None,
            None,
            None,
        )
        .await
    }

    /// One thing added twice is the mistake this guards: an exact match on
    /// name, amount, cadence, side and date is refused rather than written.
    #[tokio::test]
    async fn an_identical_cost_is_refused_rather_than_added_twice() {
        let pool = multi_connection_pool("duplicate_guard_test").await;

        add(&pool, "Car Insurance", 92.40, "2027-02-01").await.unwrap();
        let err = add(&pool, "car insurance", 92.40, "2027-02-01")
            .await
            .expect_err("the second identical add should be refused");
        assert!(err.contains("already in the list"), "unhelpful message: {err}");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM recurring_costs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 1, "the refused add must not have written a row");
    }

    /// Two real items that share a name are not duplicates — two loans both
    /// read "Loan Repayment" on the statement. Only the full match is refused.
    #[tokio::test]
    async fn a_different_amount_or_date_is_still_allowed() {
        let pool = multi_connection_pool("duplicate_allowed_test").await;

        add(&pool, "Loan Repayment", 900.00, "2027-02-01").await.unwrap();
        add(&pool, "Loan Repayment", 420.00, "2027-02-01")
            .await
            .expect("a different amount is a different item");
        add(&pool, "Loan Repayment", 900.00, "2027-02-15")
            .await
            .expect("a different date is a different item");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM recurring_costs")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, 3);
    }

    async fn account(pool: &SqlitePool) {
        sqlx::query("INSERT OR IGNORE INTO accounts (id, name, type) VALUES (1, 'Test', 'asset')")
            .execute(pool)
            .await
            .unwrap();
    }

    async fn category(pool: &SqlitePool, name: &str, parent: Option<i64>) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO categories (name, parent_id) VALUES (?, ?) RETURNING id",
        )
        .bind(name)
        .bind(parent)
        .fetch_one(pool)
        .await
        .unwrap()
    }

    /// A spend `days_ago` days back. Relative to today so the recency rule
    /// can be tested without the test rotting as the calendar moves.
    async fn spend(pool: &SqlitePool, desc: &str, amount: f64, days_ago: i64, category: Option<i64>) {
        account(pool).await;
        let date = (chrono::Local::now().naive_local().date() - chrono::Duration::days(days_ago))
            .format("%Y-%m-%d")
            .to_string();
        sqlx::query(
            "INSERT INTO transactions (account_id, category_id, date, description, debit) \
             VALUES (1, ?, ?, ?, ?)",
        )
        .bind(category)
        .bind(&date)
        .bind(desc)
        .bind(amount)
        .execute(pool)
        .await
        .unwrap();
    }

    /// A weekly supermarket run is regular without being a commitment, and
    /// offering it buries the handful of things that are. It is a filter, not
    /// a verdict, so asking for it brings it back.
    #[tokio::test]
    async fn everyday_spending_is_left_out_unless_asked_for() {
        let pool = multi_connection_pool("scan_everyday_test").await;
        let parent = category(&pool, "Food & Beverage", None).await;
        let groceries = category(&pool, "Food And Groceries", Some(parent)).await;
        for n in 0..4 {
            spend(&pool, "Corner Supermarket", 210.00, n * 7, Some(groceries)).await;
        }

        let hidden = detect_recurring(&pool, false).await.unwrap();
        assert!(hidden.is_empty(), "groceries should be skipped, got {hidden:#?}");

        let shown = detect_recurring(&pool, true).await.unwrap();
        assert_eq!(shown.len(), 1, "asking for everyday spending should bring it back");
        assert_eq!(shown[0].frequency, "Weekly");
    }

    /// A subscription cancelled last year is history. The cadence is still
    /// perfectly regular, which is exactly why recency has to be its own rule.
    #[tokio::test]
    async fn a_series_that_stopped_over_a_year_ago_is_not_offered() {
        let pool = multi_connection_pool("scan_stale_test").await;
        for n in 0..4 {
            spend(&pool, "Oldflix Subscription", 15.99, 400 + n * 30, None).await;
        }

        let items = detect_recurring(&pool, false).await.unwrap();
        assert!(items.is_empty(), "a dead series should not be offered, got {items:#?}");
    }

    /// Some banks write a different reference every month, so two
    /// subscriptions collapse into one merchant key and the amount is the
    /// only thing left that tells them apart. The key itself spans $9.99 to
    /// $89.95, so it is a mixed bag rather than a cost: splitting it is the
    /// whole point, and the average over the lot would bury both.
    #[tokio::test]
    async fn one_merchant_line_with_two_subscriptions_is_split_by_amount() {
        let pool = multi_connection_pool("scan_split_test").await;
        for n in 0..4 {
            spend(&pool, &format!("Wallet Co Australia 1234567890{n}1"), 9.99, 3 + n * 30, None).await;
            spend(&pool, &format!("Wallet Co Australia 9876543210{n}2"), 24.50, 13 + n * 30, None).await;
        }
        spend(&pool, "Wallet Co Australia 5555555555555", 79.00, 60, None).await;

        let items = detect_recurring(&pool, false).await.unwrap();
        let split: Vec<_> = items.iter().filter(|i| i.fixed_amount).collect();
        assert_eq!(split.len(), 2, "expected both subscriptions, got {items:#?}");

        // Named with the amount, because two rows reading "Paypal Australia"
        // are impossible to tell apart on the card.
        assert!(split.iter().any(|i| i.description.ends_with("$9.99")));
        assert!(split.iter().any(|i| i.description.ends_with("$24.50")));
        assert!(split.iter().all(|i| i.frequency == "Monthly" && i.occurrences == 4));

        assert!(
            !items.iter().any(|i| !i.fixed_amount),
            "the mixed-bag merchant key should not also be offered: {items:#?}"
        );
    }

    /// A pay drifts by a few dollars between periods, and every one of those
    /// figures repeats. Grouping by the exact cents made one salary look like
    /// three subscriptions; clustering within a tolerance keeps it as one.
    #[tokio::test]
    async fn a_pay_that_drifts_a_few_dollars_is_not_three_things() {
        let pool = multi_connection_pool("scan_drift_test").await;
        account(&pool).await;
        let amounts = [2100.40, 2140.55, 2150.00, 2140.55, 2100.40, 2150.00];
        for (n, amount) in amounts.iter().enumerate() {
            let date = (chrono::Local::now().naive_local().date()
                - chrono::Duration::days(n as i64 * 14))
            .format("%Y-%m-%d")
            .to_string();
            sqlx::query(
                "INSERT INTO transactions (account_id, date, description, credit)                  VALUES (1, ?, 'Shire Council Pay Abc12', ?)",
            )
            .bind(&date)
            .bind(amount)
            .execute(&pool)
            .await
            .unwrap();
        }

        let items = detect_recurring(&pool, false).await.unwrap();
        assert_eq!(items.len(), 1, "one pay, one candidate: {items:#?}");
        assert!(items[0].is_income);
        assert!(!items[0].fixed_amount);
        assert_eq!(items[0].frequency, "Fortnightly");
    }

    /// Two different loans under one "Loan Repayment" line are genuinely two
    /// things, and the average over both would be a figure you never pay.
    #[tokio::test]
    async fn two_loans_behind_one_line_are_split() {
        let pool = multi_connection_pool("scan_loans_test").await;
        for n in 0..4 {
            spend(&pool, "Loan Repayment A.100", 1800.00, 2 + n * 30, None).await;
            spend(&pool, "Loan Repayment B.200", 640.00, 16 + n * 30, None).await;
        }

        let items = detect_recurring(&pool, false).await.unwrap();
        assert_eq!(items.len(), 2, "expected both loans: {items:#?}");
        assert!(items.iter().all(|i| i.fixed_amount));
        assert!(items.iter().any(|i| (i.avg_amount - 1800.00).abs() < 0.01));
        assert!(items.iter().any(|i| (i.avg_amount - 640.00).abs() < 0.01));
    }

    /// The ordinary case still reads as one thing: a merchant that charges
    /// the same amount every month is one candidate, not one per amount.
    #[tokio::test]
    async fn a_steady_merchant_is_still_a_single_candidate() {
        let pool = multi_connection_pool("scan_steady_test").await;
        for n in 0..4 {
            spend(&pool, "Streamflix Monthly", 18.99, n * 30, None).await;
        }

        let items = detect_recurring(&pool, false).await.unwrap();
        assert_eq!(items.len(), 1, "expected one candidate, got {items:#?}");
        assert!(!items[0].fixed_amount);
        assert_eq!(items[0].avg_amount, 18.99);
    }

    /// The pair the rowid bug left behind: same thing, added twice, and only
    /// one copy rolled forward to its next date. Different dates must not
    /// hide it, or the review would miss exactly what it is there for.
    #[tokio::test]
    async fn two_copies_of_one_thing_are_grouped_even_on_different_dates() {
        let pool = multi_connection_pool("duplicate_find_test").await;

        add(&pool, "Northfield Care Wages", 1450.00, "2027-01-06").await.unwrap();
        add(&pool, "Northfield Care Wages 0092", 1450.00, "2027-01-20").await.unwrap();

        let groups = duplicate_groups(&pool).await.unwrap();
        assert_eq!(groups.len(), 1, "expected one group, got {groups:#?}");
        assert_eq!(groups[0].items.len(), 2);
        assert_eq!(groups[0].amount, 1450.00);
    }

    /// Nothing to report is the normal case, and two items that merely share
    /// a merchant key are not a duplicate — the amount has to agree too.
    #[tokio::test]
    async fn items_that_only_share_a_name_are_not_reported() {
        let pool = multi_connection_pool("duplicate_find_clean_test").await;

        add(&pool, "Loan Repayment A.100", 900.00, "2027-02-01").await.unwrap();
        add(&pool, "Loan Repayment B.200", 420.00, "2027-02-01").await.unwrap();
        add(&pool, "Car Insurance", 92.40, "2027-02-01").await.unwrap();

        let groups = duplicate_groups(&pool).await.unwrap();
        assert!(groups.is_empty(), "nothing should be flagged, got {groups:#?}");
    }

    /// A pay and a refund from the same merchant at the same amount are two
    /// sides of the ledger, not one row twice.
    #[tokio::test]
    async fn the_two_sides_of_a_merchant_are_not_a_duplicate() {
        let pool = multi_connection_pool("duplicate_find_sides_test").await;

        add(&pool, "Acme Pty Ltd", 120.00, "2027-03-01").await.unwrap();
        insert_recurring_cost(
            &pool,
            "Acme Pty Ltd",
            120.00,
            "Monthly",
            Some(KIND_INCOME.to_string()),
            None,
            Some("2027-03-01".to_string()),
            None,
            None,
            None,
        )
        .await
        .unwrap();

        assert!(duplicate_groups(&pool).await.unwrap().is_empty());
    }

    /// Adding a cost has to return the row that was just written, whichever
    /// connection the pool happens to hand out. Reading the id back with a
    /// second statement (`SELECT last_insert_rowid()`) answered 0 on a
    /// connection that had inserted nothing, and the read-back then failed
    /// with "no rows returned by a query that expected to return at least one
    /// row" — which is what "Add" in the scan dialog did.
    #[tokio::test]
    async fn adding_a_cost_returns_the_row_it_wrote() {
        let pool = multi_connection_pool("insert_rowid_test").await;

        for n in 1..=4 {
            let name = format!("Bill {n}");
            let cost = insert_recurring_cost(
                &pool,
                &name,
                100.0 + n as f64,
                "Monthly",
                Some(KIND_EXPENSE.to_string()),
                None,
                Some("2027-01-15".to_string()),
                None,
                None,
                None,
            )
            .await
            .unwrap_or_else(|e| panic!("insert {n} failed: {e}"));

            assert_eq!(cost.name, name);
            assert_eq!(cost.amount, 100.0 + n as f64);

            let stored: String = sqlx::query_scalar("SELECT name FROM recurring_costs WHERE id = ?")
                .bind(cost.id)
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|e| panic!("id {} from insert {n} is not a real row: {e}", cost.id));
            assert_eq!(stored, name);
        }
    }
}
