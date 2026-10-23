import { mergeConfig } from "vite";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import base from "../../vite.config.ts";
export default mergeConfig(base, {
 cacheDir: join(tmpdir(), "tokage-runtime-layout-" + createHash("sha256").update(import.meta.url).digest("hex").slice(0, 12)),
 resolve: { alias: [{ find: /^(?:(?:\.\.\/)+lib\/runtime|\.\/runtime)$/, replacement: fileURLToPath(new URL("./runtime.fixture.ts", import.meta.url)) }] },
 server: { host: "127.0.0.1", port: 1533, strictPort: true },
});
