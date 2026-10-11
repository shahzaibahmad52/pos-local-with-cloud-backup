//! Running balances for khata (customers) and payables (suppliers).
//!
//! Payments settle the oldest charges first, so what is still unpaid is
//! always the newest charges. That gives the age of a debt ("owes for 43
//! days") and which supplier invoices are still due.

/// A charge (amount > 0) or a payment (amount < 0), oldest first.
#[derive(Debug, Clone)]
pub struct Entry {
    pub amount: i64,
    /// `YYYY-MM-DD` the charge was made.
    pub date: String,
    /// When a charge falls due (`YYYY-MM-DD`), if it has terms.
    pub due: Option<String>,
    /// The bill a charge is for, or a refund comes back from. A refund
    /// settles its own bill first, so ageing follows what was returned.
    pub bill: Option<String>,
}

/// A charge with the part of it still unpaid.
#[derive(Debug, Clone, PartialEq)]
pub struct Open {
    pub index: usize,
    pub unpaid: i64,
    pub date: String,
    pub due: Option<String>,
}

/// Charges not yet covered by payments, oldest first. A payment larger than
/// everything owed leaves nothing open (the extra is an advance).
pub fn open_charges(entries: &[Entry]) -> Vec<Open> {
    let mut open: Vec<Open> = Vec::new();
    let mut credit: i64 = 0;
    for (index, e) in entries.iter().enumerate() {
        if e.amount > 0 {
            // An advance paid earlier covers new charges first.
            let used = credit.min(e.amount);
            credit -= used;
            if e.amount > used {
                open.push(Open { index, unpaid: e.amount - used, date: e.date.clone(), due: e.due.clone() });
            }
        } else {
            let mut pay = -e.amount;
            if let Some(bill) = &e.bill {
                if let Some(o) = open
                    .iter_mut()
                    .find(|o| entries[o.index].amount > 0 && entries[o.index].bill.as_ref() == Some(bill))
                {
                    let take = pay.min(o.unpaid);
                    o.unpaid -= take;
                    pay -= take;
                }
            }
            for o in open.iter_mut() {
                if pay == 0 {
                    break;
                }
                let take = pay.min(o.unpaid);
                o.unpaid -= take;
                pay -= take;
            }
            open.retain(|o| o.unpaid > 0);
            credit += pay;
        }
    }
    open
}

/// Whole days from `from` to `to` (both `YYYY-MM-DD`); 0 if either is unreadable.
pub fn days_between(from: &str, to: &str) -> i64 {
    let parse = |s: &str| chrono::NaiveDate::parse_from_str(s.get(..10).unwrap_or(s), "%Y-%m-%d").ok();
    match (parse(from), parse(to)) {
        (Some(a), Some(b)) => (b - a).num_days(),
        _ => 0,
    }
}

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn add_days(date: &str, days: i64) -> Option<String> {
    let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    Some((d + chrono::Duration::days(days)).format("%Y-%m-%d").to_string())
}

pub fn valid_date(s: &str) -> bool {
    s.len() == 10 && chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(amount: i64, date: &str) -> Entry {
        Entry { amount, date: date.into(), due: None, bill: None }
    }

    #[test]
    fn a_refund_settles_the_bill_it_came_back_from() {
        let bill =
            |amount: i64, date: &str, b: &str| Entry { amount, date: date.into(), due: None, bill: Some(b.into()) };
        // An old bill and a new one; the new one is returned.
        let open = open_charges(&[
            bill(1_000, "2026-08-01", "a"),
            bill(500, "2026-10-01", "b"),
            bill(-500, "2026-10-02", "b"),
        ]);
        assert_eq!(open.len(), 1);
        assert_eq!((open[0].date.as_str(), open[0].unpaid), ("2026-08-01", 1_000));
    }

    #[test]
    fn payments_settle_oldest_first() {
        let open = open_charges(&[
            e(4200, "2026-08-12"),
            e(-4000, "2026-08-28"),
            e(3900, "2026-09-05"),
            e(4350, "2026-09-18"),
        ]);
        assert_eq!(open.len(), 3);
        assert_eq!(open[0].unpaid, 200);
        assert_eq!(open[0].date, "2026-08-12");
        assert_eq!(open.iter().map(|o| o.unpaid).sum::<i64>(), 200 + 3900 + 4350);
    }

    #[test]
    fn advance_covers_later_charges() {
        let open = open_charges(&[e(-1000, "2026-10-01"), e(600, "2026-10-02"), e(600, "2026-10-03")]);
        assert_eq!(open, vec![Open { index: 2, unpaid: 200, date: "2026-10-03".into(), due: None }]);
    }

    #[test]
    fn settled_leaves_nothing() {
        assert!(open_charges(&[e(500, "2026-10-01"), e(-500, "2026-10-02")]).is_empty());
    }

    #[test]
    fn dates() {
        assert_eq!(days_between("2026-08-28", "2026-10-10"), 43);
        assert_eq!(days_between("2026-10-10T09:00:00+05:00", "2026-10-10"), 0);
        assert_eq!(add_days("2026-10-10", 30).as_deref(), Some("2026-11-09"));
        assert!(valid_date("2026-02-28"));
        assert!(!valid_date("2026-02-30"));
    }
}
