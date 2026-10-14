pub fn total(prices: &[i64]) -> i64 {
    prices.iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        assert_eq!(total(&[1, 2]), 3);
        assert_eq!(total(&[]), 0);
    }

    #[test]
    fn half_cent() {
        assert_eq!(total(&[5]), 5);
    }
}
