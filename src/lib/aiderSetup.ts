export type AiderSetupPlatform = "macos" | "linux" | "windows";

export const AIDER_KEY_PROVIDERS = [
  { name: "DeepSeek", key: "DEEPSEEK_API_KEY" },
  { name: "OpenAI", key: "OPENAI_API_KEY" },
  { name: "Anthropic", key: "ANTHROPIC_API_KEY" },
  { name: "Gemini", key: "GEMINI_API_KEY" },
  { name: "OpenRouter", key: "OPENROUTER_API_KEY" },
  { name: "Groq", key: "GROQ_API_KEY" },
  { name: "Mistral", key: "MISTRAL_API_KEY" },
  { name: "xAI", key: "XAI_API_KEY" },
  { name: "Cohere", key: "COHERE_API_KEY" },
] as const;

export function aiderSetupPlatform(): AiderSetupPlatform {
  const platform = `${navigator.platform} ${navigator.userAgent}`;
  if (/win(dows|32|64)/i.test(platform)) return "windows";
  return /mac/i.test(platform) ? "macos" : "linux";
}

/** Adds a missing placeholder only; the user enters their real key in the editor. */
export function aiderKeySetupScript(platform: AiderSetupPlatform, key: typeof AIDER_KEY_PROVIDERS[number]["key"]): string {
  if (platform === "windows") {
    return `$keyFile = Join-Path $HOME '.env'
$utf8 = [System.Text.UTF8Encoding]::new($false)
if (-not (Test-Path -LiteralPath $keyFile)) {
    [System.IO.File]::WriteAllText($keyFile, '', $utf8)
}
$content = [System.IO.File]::ReadAllText($keyFile)
if ($content -notmatch '(?m)^\\s*(export\\s+)?${key}\\s*=') {
    $line = [Environment]::NewLine + '${key}="YOUR_${key}"' + [Environment]::NewLine
    [System.IO.File]::AppendAllText($keyFile, $line, $utf8)
}
notepad.exe $keyFile`;
  }
  const editor = platform === "macos" ? 'open -e "$key_file"' : `if command -v xdg-open >/dev/null 2>&1; then
  xdg-open "$key_file" || vi "$key_file"
else
  vi "$key_file"
fi`;
  return `(
set -e
key_file="$HOME/.env"
touch "$key_file"
chmod 600 "$key_file"
if ! grep -Eq '^[[:space:]]*(export[[:space:]]+)?${key}[[:space:]]*=' "$key_file"; then
  printf '\\n${key}="YOUR_${key}"\\n' >> "$key_file"
fi
${editor}
)`;
}
