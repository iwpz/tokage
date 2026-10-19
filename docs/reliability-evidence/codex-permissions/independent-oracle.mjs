import assert from 'node:assert/strict';
const part = process.argv[2];
if (part === 'pricing' || part === 'all') {
  const {subtotal} = await import('./pricing.mjs');
  assert.equal(subtotal([]), 0);
  assert.equal(subtotal([{priceCents:125, quantity:2},{priceCents:50, quantity:3}]), 400);
  assert.equal(subtotal([{priceCents:0, quantity:9}]), 0);
  for (const item of [{priceCents:-1,quantity:1},{priceCents:1.5,quantity:1},{priceCents:1,quantity:-1},{priceCents:1,quantity:0.5}]) assert.throws(() => subtotal([item]), RangeError);
}
if (part === 'format' || part === 'all') {
  const {money} = await import('./format.mjs');
  for (const [n,s] of [[0,'$0.00'],[1,'$0.01'],[105,'$1.05'],[-105,'-$1.05'],[120000,'$1200.00']]) assert.equal(money(n),s);
}
if (part === 'integration' || part === 'all') {
  const {invoice} = await import('./invoice.mjs');
  assert.equal(invoice([]), 'Total: $0.00');
  assert.equal(invoice([{priceCents:125,quantity:2},{priceCents:50,quantity:3}]), 'Total: $4.00');
  assert.throws(() => invoice([{priceCents:-1,quantity:2}]), RangeError);
}
assert.ok(['pricing','format','integration','all'].includes(part));
console.log('TOKAGE_NATURAL_' + part.toUpperCase() + '_OK');
