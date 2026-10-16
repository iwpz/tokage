"""Mutation checks for m2b's independent oracle; no paid calls or desktop required."""
import subprocess
import tempfile
import unittest
from pathlib import Path
from live_natural_fixture import ORACLE

GOOD = {
 'pricing.mjs': 'export function subtotal(items) {return items.reduce((n,{priceCents:p,quantity:q})=>{if(!Number.isInteger(p)||!Number.isInteger(q)||p<0||q<0)throw new RangeError();return n+p*q;},0)}',
 'format.mjs': 'export function money(c){return (c<0?"-":"")+"$"+(Math.abs(c)/100).toFixed(2)}',
 'invoice.mjs': 'import {subtotal} from "./pricing.mjs";import {money} from "./format.mjs";export function invoice(items){return "Total: "+money(subtotal(items))}',
}
class OracleMutationTests(unittest.TestCase):
 def run_oracle(self, replacements):
  with tempfile.TemporaryDirectory() as directory:
   root=Path(directory)
   for name, text in {**GOOD, **replacements, 'oracle.mjs':ORACLE}.items(): (root/name).write_text(text)
   return subprocess.run(['node','oracle.mjs','all'],cwd=root,capture_output=True).returncode
 def test_correct_contract(self): self.assertEqual(self.run_oracle({}),0)
 def test_rejects_seeded_arithmetic_format_and_integration_defects(self):
  for name, text in {
   'pricing.mjs':GOOD['pricing.mjs'].replace('n+p*q','n+p+q'),
   'format.mjs':GOOD['format.mjs'].replace('(c<0?"-":"")','""'),
   'invoice.mjs':GOOD['invoice.mjs'].replace('money(subtotal(items))','"$4.00"'),
  }.items():
   with self.subTest(name=name): self.assertNotEqual(self.run_oracle({name:text}),0)
 def test_rejects_removed_boundary_validation(self):
  self.assertNotEqual(self.run_oracle({'pricing.mjs':'export function subtotal(items){return items.reduce((n,i)=>n+i.priceCents*i.quantity,0)}'}),0)
 def test_repository_test_weakening_cannot_bypass_external_oracle(self):
  self.assertNotEqual(self.run_oracle({'pricing.mjs':'export function subtotal(){return 0}', 'check.mjs':'process.exit(0)', 'package.json':'{"scripts":{"test":"true"}}'}),0)
if __name__=='__main__': unittest.main()
