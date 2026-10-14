export function total(prices: number[]): number {
  // @ts-ignore
  return prices.reduce((a, b) => a + b, 0);
}
