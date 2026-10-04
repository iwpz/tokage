import { memo, useState, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import rehypeSanitize from "rehype-sanitize";
import { useTranslation } from "react-i18next";
import { Check, Copy } from "lucide-react";
import { cn } from "../lib/cn";
import { openExternalUrl } from "../lib/ipc";

/**
 * Model output as GitHub-flavoured Markdown. Raw HTML is never rendered (docs/10 §5:
 * rehype-sanitize). Web links open in the browser; links to local files call `onOpenFile`.
 */
export const Markdown = memo(function Markdown({
  text,
  onOpenFile,
  className,
}: {
  text: string;
  onOpenFile?: (path: string) => void;
  className?: string;
}) {
  const components: Components = {
    a: ({ href, children }) => <Link href={href ?? ""} onOpenFile={onOpenFile}>{children}</Link>,
    pre: ({ children }) => <CodeBlock>{children}</CodeBlock>,
    code: ({ className: c, children }) => {
      // Fenced blocks carry a language class and are wrapped by `pre`; the rest is inline.
      const block = /language-/.test(c ?? "") || String(children).includes("\n");
      return block ? (
        <code className={c}>{children}</code>
      ) : (
        <code className="rounded bg-hover px-1 py-px font-mono text-[0.9em]">{children}</code>
      );
    },
    h1: ({ children }) => <h3 className="mt-4 mb-2 text-[17px] font-semibold first:mt-0">{children}</h3>,
    h2: ({ children }) => <h3 className="mt-4 mb-2 text-[15px] font-semibold first:mt-0">{children}</h3>,
    h3: ({ children }) => <h4 className="mt-3 mb-1.5 text-[14px] font-semibold first:mt-0">{children}</h4>,
    h4: ({ children }) => <h5 className="mt-3 mb-1 font-semibold first:mt-0">{children}</h5>,
    p: ({ children }) => <p className="my-2 first:mt-0 last:mb-0">{children}</p>,
    ul: ({ children, className: c }) => (
      <ul className={cn("my-2 space-y-1 pl-5", c?.includes("contains-task-list") ? "list-none pl-1" : "list-disc")}>{children}</ul>
    ),
    ol: ({ children, start }) => (
      <ol start={start} className="my-2 list-decimal space-y-1 pl-5">
        {children}
      </ol>
    ),
    li: ({ children }) => <li className="pl-0.5 marker:text-subtle">{children}</li>,
    blockquote: ({ children }) => <blockquote className="my-2 border-l-2 border-border pl-3 text-muted">{children}</blockquote>,
    hr: () => <hr className="my-4 border-border" />,
    table: ({ children }) => (
      <div className="my-2 overflow-x-auto">
        <table className="w-full border-collapse text-[13px]">{children}</table>
      </div>
    ),
    th: ({ children, style }) => (
      <th style={style} className="border border-border bg-hover px-2.5 py-1.5 text-left font-medium">
        {children}
      </th>
    ),
    td: ({ children, style }) => (
      <td style={style} className="border border-border px-2.5 py-1.5 align-top">
        {children}
      </td>
    ),
    input: ({ checked }) => (
      <input type="checkbox" checked={!!checked} readOnly disabled className="mr-1.5 align-middle accent-[var(--accent)]" />
    ),
    img: ({ alt }) => <span className="text-muted">[{alt || "image"}]</span>,
  };

  return (
    <div data-selectable className={cn("leading-relaxed break-words", className)}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} rehypePlugins={[rehypeSanitize]} components={components}>
        {text}
      </ReactMarkdown>
    </div>
  );
});

/** Local file targets models emit: absolute paths, `file://` URLs, or relative paths with an extension. */
function localPath(href: string): string | null {
  if (/^file:\/\//i.test(href)) return decodeURIComponent(href.replace(/^file:\/\//i, ""));
  if (/^[a-z][a-z0-9+.-]*:/i.test(href) || href.startsWith("#")) return null;
  return decodeURIComponent(href).replace(/:\d+(:\d+)?$/, "");
}

function Link({ href, children, onOpenFile }: { href: string; children: ReactNode; onOpenFile?: (p: string) => void }) {
  const web = /^https?:\/\//i.test(href);
  const file = web ? null : localPath(href);
  return (
    <a
      href={href}
      title={href}
      onClick={(e) => {
        e.preventDefault();
        if (web) void openExternalUrl(href);
        else if (file && onOpenFile) onOpenFile(file);
      }}
      className="text-accent underline decoration-accent/40 underline-offset-2 hover:decoration-accent"
    >
      {children}
    </a>
  );
}

function CodeBlock({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  // The language class sits on the inner <code>.
  const child = (Array.isArray(children) ? children[0] : children) as { props?: { className?: string; children?: ReactNode } };
  const lang = /language-([\w+-]+)/.exec(child?.props?.className ?? "")?.[1];
  const code = String(child?.props?.children ?? "").replace(/\n$/, "");

  return (
    <div className="group/code my-2 overflow-hidden rounded-lg border border-border bg-sidebar">
      <div className="flex h-7 items-center justify-between border-b border-border px-3 text-xs text-subtle">
        <span className="font-mono">{lang ?? ""}</span>
        <button
          type="button"
          onClick={() => {
            void navigator.clipboard.writeText(code).then(() => {
              setCopied(true);
              setTimeout(() => setCopied(false), 1500);
            });
          }}
          className="inline-flex items-center gap-1 rounded px-1 hover:text-fg"
        >
          {copied ? <Check aria-hidden className="size-3" /> : <Copy aria-hidden className="size-3" />}
          {copied ? t("workspace.chat.copied") : t("workspace.chat.copy")}
        </button>
      </div>
      <pre className="overflow-x-auto px-3 py-2.5 font-mono text-[12.5px] leading-relaxed">{children}</pre>
    </div>
  );
}
