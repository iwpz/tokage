#!/usr/bin/env node
// i18n guard. Fails when:
//  1. en/zh locale keys differ, or an English value contains CJK;
//  2. a t()/lm()/lmRef()/lmText()/i18n.t() key is missing from the locale;
//  3. a TS/TSX string, template or JSX text outside the locale files contains CJK;
//  4. a Rust string outside tests contains CJK.
// Intentional exceptions live in scripts/i18n-allowlist.txt.
import { execSync } from "node:child_process";
import { createRequire } from "node:module";
import fs from "node:fs";

const require = createRequire(import.meta.url);
const { parseSync, transformSync } = require("rolldown/utils");
const CJK = /[一-鿿　-〿！-～]/;
const sh = (c) => execSync(c, { encoding: "utf8" }).split("\n").filter(Boolean);
const errors = [];

const allow = fs
  .readFileSync("scripts/i18n-allowlist.txt", "utf8")
  .split("\n")
  .filter((l) => l.trim() && !l.startsWith("#"))
  .map((l) => l.split("|"));
const allowed = (file, line) =>
  line.includes("i18n-ignore") || allow.some(([p, s]) => p === file && (!s || line.includes(s)));

// --- 1 & 2: locales -------------------------------------------------------
function load(p) {
  const code = transformSync(p, fs.readFileSync(p, "utf8"), { lang: "ts" })
    .code.replace(/export default (\w+);?/, "module.exports=$1;")
    .replace(/^export .*$/gm, "");
  const m = { exports: {} };
  new Function("module", "exports", code)(m, m.exports);
  return m.exports;
}
const flat = (o, pre = "") =>
  Object.entries(o).reduce(
    (r, [k, v]) => (typeof v === "string" ? { ...r, [pre + k]: v } : { ...r, ...flat(v, pre + k + ".") }),
    {},
  );
const en = flat(load("src/i18n/locales/en.ts"));
const zh = flat(load("src/i18n/locales/zh.ts"));
for (const k of Object.keys(en)) if (!(k in zh)) errors.push(`locale: "${k}" missing in zh`);
for (const k of Object.keys(zh)) if (!(k in en)) errors.push(`locale: "${k}" missing in en`);
for (const [k, v] of Object.entries(en)) if (CJK.test(v)) errors.push(`locale: en value of "${k}" contains CJK`);
const has = (k) => k in en || ["_one", "_other", "_zero", "_two", "_few", "_many"].some((s) => k + s in en);

// --- 2 & 3: TS / TSX ------------------------------------------------------
const tsFiles = sh("git ls-files -co --exclude-standard src").filter(
  (f) => /\.tsx?$/.test(f) && !f.includes("i18n/locales") && !/\.test\./.test(f),
);
function walk(n, cb) {
  if (!n || typeof n !== "object") return;
  if (Array.isArray(n)) return n.forEach((x) => walk(x, cb));
  if (n.type) cb(n);
  for (const k in n) if (n[k] && typeof n[k] === "object") walk(n[k], cb);
}
for (const f of tsFiles) {
  const src = fs.readFileSync(f, "utf8");
  const lines = src.split("\n");
  const lineOf = (o) => src.slice(0, o).split("\n").length;
  for (const m of src.matchAll(/\b(?:t|lm|lmRef|lmText|i18n\.t)\(\s*["']([\w.\-]+)["']/g)) {
    if (m[1].includes(".") && !has(m[1])) errors.push(`${f}:${lineOf(m.index)}: unknown i18n key "${m[1]}"`);
  }
  const ast = parseSync(f, src, { lang: f.endsWith("x") ? "tsx" : "ts" }).program;
  walk(ast, (n) => {
    let text = null;
    if (n.type === "Literal" && typeof n.value === "string") text = n.value;
    else if (n.type === "TemplateElement") text = n.value.cooked ?? n.value.raw;
    else if (n.type === "JSXText") text = n.value;
    if (text && CJK.test(text)) {
      // Report the line of the first CJK character (JSX text nodes often start on an earlier line).
      const ln = lineOf(n.start + Math.max(0, src.slice(n.start, n.end).search(CJK)));
      if (!allowed(f, lines[ln - 1])) errors.push(`${f}:${ln}: hardcoded CJK string: ${text.trim().slice(0, 40)}`);
    }
  });
}

// --- 4: Rust --------------------------------------------------------------
const rsFiles = sh("git ls-files -co --exclude-standard src-tauri").filter(
  (f) => f.endsWith(".rs") && !/\/tests?\//.test(f) && !/(^|\/)(tests?|.*_tests|acceptance)\.rs$/.test(f),
);
for (const f of rsFiles) {
  let inTest = false;
  fs.readFileSync(f, "utf8").split("\n").forEach((line, i) => {
    const s = line.trim();
    if (s.startsWith("#[cfg(test)]")) inTest = true;
    if (inTest || s.startsWith("//")) return;
    const code = line.replace(/\s\/\/.*$/, "");
    if (CJK.test(code) && !allowed(f, line)) errors.push(`${f}:${i + 1}: hardcoded CJK in Rust: ${s.slice(0, 50)}`);
  });
}

if (errors.length) {
  console.error(errors.join("\n") + `\n\n${errors.length} i18n problem(s).`);
  process.exit(1);
}
console.log(`i18n OK (${Object.keys(en).length} keys, ${tsFiles.length} TS and ${rsFiles.length} Rust files checked)`);
