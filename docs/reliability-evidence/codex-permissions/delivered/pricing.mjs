export function subtotal(items) {
  let total = 0;

  for (const { priceCents, quantity } of items) {
    if (!Number.isInteger(priceCents) || priceCents < 0) {
      throw new RangeError("priceCents must be a non-negative integer");
    }
    if (!Number.isInteger(quantity) || quantity < 0) {
      throw new RangeError("quantity must be a non-negative integer");
    }

    total += priceCents * quantity;
  }

  return total;
}
