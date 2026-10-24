import type { AccountUsageResult, ProviderInfo } from "./ipc";

/** Apply the CLI status proof bundled with this accepted source revision, never infer login from quota. */
export function applySourceLogin(provider: ProviderInfo, result: AccountUsageResult): ProviderInfo {
  const proof = result.sourceLogin;
  if (provider.id !== "CURSOR_CLI" || !proof || provider.path !== proof.path ||
      !["READY", "UNKNOWN_AUTH", "NEEDS_LOGIN"].includes(provider.status)) return provider;
  if (provider.status === "NEEDS_LOGIN" && proof.status === "READY") return provider;
  const account = proof.account ? { ...proof.account,
    plan: proof.account.plan ?? (provider.authMethod === proof.authMethod ? provider.account?.plan : null) ?? null,
    usage: result.status === "LATEST" ? result.latestSuccess?.display ?? null : null,
  } : null;
  const guidance = (provider.guidance ?? []).filter((g) => g.code !== "LOGIN" && g.code !== "VERIFY_LOGIN");
  if (proof.status === "NEEDS_LOGIN") guidance.push({ code: "LOGIN", command: "cursor-agent login" });
  return { ...provider, status: proof.status, authMethod: proof.authMethod, account, guidance };
}
