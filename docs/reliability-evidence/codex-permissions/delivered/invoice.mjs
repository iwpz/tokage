import { subtotal } from "./pricing.mjs";
import { money } from "./format.mjs";

export function invoice(items) {
  return `Total: ${money(subtotal(items))}`;
}
