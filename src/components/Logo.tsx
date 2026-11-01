export function Logo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 512 512" aria-hidden className={className}>
      <defs>
        <linearGradient id="tokage-mark" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="color-mix(in srgb, var(--accent) 88%, white)" />
          <stop offset="100%" stopColor="color-mix(in srgb, var(--accent) 92%, black)" />
        </linearGradient>
      </defs>
      <rect width="512" height="512" rx="124" fill="url(#tokage-mark)" />
      <rect x="8" y="8" width="496" height="496" rx="116" fill="none" stroke="rgba(255,255,255,0.22)" strokeWidth="16" />
      <path
        d="M136 96L376 96A18 18 0 0 1 394 114L394 114A18 18 0 0 1 376 132L339.5 132A6 6 0 0 0 334.3 135.1L267.2 254.1A8 8 0 0 0 267.2 261.9L334.3 380.9A6 6 0 0 0 339.5 384L376 384A18 18 0 0 1 394 402L394 402A18 18 0 0 1 376 420L327.8 420A6 6 0 0 1 322.6 416.9L312.5 399.1A6 6 0 0 0 307.3 396L204.7 396A6 6 0 0 0 199.5 399.1L189.4 416.9A6 6 0 0 1 184.2 420L136 420A18 18 0 0 1 118 402L118 402A18 18 0 0 1 136 384L172.5 384A6 6 0 0 0 177.7 380.9L244.8 261.9A8 8 0 0 0 244.8 254.1L177.7 135.1A6 6 0 0 0 172.5 132L136 132A18 18 0 0 1 118 114L118 114A18 18 0 0 1 136 96ZM260.4 306.5L293 364.5A5 5 0 0 1 288.7 372L223.3 372A5 5 0 0 1 219 364.5L251.6 306.5A5 5 0 0 1 260.4 306.5Z"
        fill="var(--accent-fg)"
        fillRule="evenodd"
      />
    </svg>
  );
}
