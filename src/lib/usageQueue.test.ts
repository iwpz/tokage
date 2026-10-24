import { expect, it } from "vitest";
import { createUsageQueue } from "./usageQueue";
it("bounds concurrent IPC calls and releases queued work exactly once",async()=>{
 const q=createUsageQueue();const a=await q.acquire();const b=await q.acquire();let started=false;
 const c=q.acquire().then(release=>{started=true;return release;});await Promise.resolve();expect(started).toBe(false);
 a();a();const release=await c;expect(started).toBe(true);b();release();
});
