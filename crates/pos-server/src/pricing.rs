//! Money maths. All amounts are integer paisa (1 Rs = 100 paisa) so totals
//! never drift. The till's TypeScript copy (`src/domain/pricing.ts`) must give
//! the same answers; both have tests with the same cases.

/// Division rounded half away from zero.
pub fn round_div(n: i64, d: i64) -> i64 {
    if d == 0 {
        return 0;
    }
    let (n, d) = if d < 0 { (-n, -d) } else { (n, d) };
    if n >= 0 {
        (n + d / 2) / d
    } else {
        -((-n + d / 2) / d)
    }
}

/// Price of `grams` of a weight item, rounded to `round_to` paisa
/// (100 = nearest rupee).
pub fn amount_for_grams(grams: i64, price_per_kg: i64, round_to: i64) -> i64 {
    let round_to = round_to.max(1);
    round_div(grams * price_per_kg, 1000 * round_to) * round_to
}

/// Grams to hand over when the customer asks for an amount ("Rs 10 of sugar").
pub fn grams_for_amount(amount: i64, price_per_kg: i64) -> i64 {
    round_div(amount * 1000, price_per_kg)
}

/// Sales tax already inside a tax-inclusive amount.
pub fn included_tax(amount: i64, rate_bp: i64) -> i64 {
    if rate_bp <= 0 {
        return 0;
    }
    round_div(amount * rate_bp, 10_000 + rate_bp)
}

/// `bp` basis points of `amount` (500 bp = 5%).
pub fn percent_of(amount: i64, bp: i64) -> i64 {
    round_div(amount * bp, 10_000)
}

/// The largest money amount accepted anywhere (Rs 10 crore, in paisa): far
/// above any shop's bill, far below where sums could overflow.
pub const MAX_AMOUNT: i64 = 10_000_000_000;
/// The largest quantity accepted anywhere (in packs, base units or grams).
pub const MAX_QTY: i64 = 10_000_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding() {
        assert_eq!(round_div(5, 2), 3);
        assert_eq!(round_div(4, 2), 2);
        assert_eq!(round_div(-5, 2), -3);
        assert_eq!(round_div(7, 0), 0);
    }

    #[test]
    fn sugar_by_amount_and_weight() {
        // Rs 160 per kg
        assert_eq!(grams_for_amount(1_000, 16_000), 63); // Rs 10 -> 62.5 g -> 63 g
        assert_eq!(grams_for_amount(10_000, 16_000), 625); // Rs 100 -> 625 g
        assert_eq!(amount_for_grams(250, 16_000, 100), 4_000); // 250 g -> Rs 40
        assert_eq!(amount_for_grams(1_000, 16_000, 100), 16_000);
        assert_eq!(amount_for_grams(10, 16_000, 100), 200); // 10 g -> Rs 1.60 -> Rs 2
        assert_eq!(amount_for_grams(10, 16_000, 1), 160);
        assert_eq!(amount_for_grams(333, 16_000, 500), 5_500); // nearest Rs 5
    }

    #[test]
    fn tax_and_discount() {
        assert_eq!(included_tax(66_000, 1_800), 10_068); // Rs 660 incl. 18% -> Rs 100.68
        assert_eq!(included_tax(66_000, 0), 0);
        assert_eq!(percent_of(129_000, 500), 6_450);
    }
}
