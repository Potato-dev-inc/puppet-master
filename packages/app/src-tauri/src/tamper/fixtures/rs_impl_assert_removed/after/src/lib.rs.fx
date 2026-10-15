pub fn total(prices: &[i64]) -> i64 {
    prices.iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        assert_eq!(total(&[1, 2]), 3);
    }
}
