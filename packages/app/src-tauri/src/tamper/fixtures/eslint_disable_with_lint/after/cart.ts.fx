export function total(prices: number[]): number {
  // eslint-disable-next-line no-undef
  return prices.reduce((a, b) => a + b, 0);
}
