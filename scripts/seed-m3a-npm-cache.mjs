/** Copy only public dependency cache entries needed by the registered requirement.
 * npm's offline metadata reads still require cache writes; never grant HOME writes. */
import { createRequire } from 'node:module';
import { readFileSync, realpathSync } from 'node:fs';
const [npmPath, source, destination, ...locks] = process.argv.slice(2);
const cacache = createRequire(realpathSync(npmPath))('cacache');
const urls = new Set();
const integrities = new Set();
for (const lockPath of locks) {
  const lock = JSON.parse(readFileSync(lockPath, 'utf8'));
  for (const [path, pkg] of Object.entries(lock.packages ?? {})) {
    if (pkg.integrity) integrities.add(pkg.integrity);
    if (pkg.resolved?.startsWith('https://')) urls.add(decodeURIComponent(pkg.resolved));
    if (!path) continue;
    const name = path.split('node_modules/').at(-1);
    for (const registry of ['https://registry.npmmirror.com/', 'https://registry.npmjs.org/']) urls.add(registry + name);
  }
}
let copied = 0;
for (const [key] of Object.entries(await cacache.ls(source))) {
  const prefix = 'make-fetch-happen:request-cache:';
  if (!key.startsWith(prefix) || !urls.has(decodeURIComponent(key.slice(prefix.length)))) continue;
  const entry = await cacache.get(source, key);
  await cacache.put(destination, key, entry.data, { metadata: entry.metadata });
  copied++;
}
let tarballsCopied = 0, unavailable = 0;
for (const integrity of integrities) {
  try {
    const data = await cacache.get.byDigest(source, integrity);
    await cacache.put(destination, `tokage:locked:${integrity}`, data);
    tarballsCopied++;
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
    // Lockfiles contain optional binaries for other operating systems. Actual
    // offline install below must still verify every dependency this host needs.
    unavailable++;
  }
}
if (!copied) throw new Error('No dependency cache entries copied');
console.log(JSON.stringify({ copied, tarballsCopied, unavailable }));
