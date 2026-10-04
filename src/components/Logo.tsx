export function Logo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 512 512" aria-hidden className={className}>
      <rect width="512" height="512" rx="112" fill="var(--accent)" />
      <path
        d="M256 104c-44 0-76 30-76 70 0 26 14 46 34 58l-58 52c-16 14-16 38 0 52l100 90 100-90c16-14 16-38 0-52l-58-52c20-12 34-32 34-58 0-40-32-70-76-70z"
        fill="var(--accent-fg)"
      />
    </svg>
  );
}
