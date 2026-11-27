// Tiny inline formatter for the FAQ and wiki copy: `code`, **bold** and [text](url).
// The input is our own text from faq.ts / wiki.ts; it is escaped first, so nothing is trusted as HTML.

const escapeHtml = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

export function inline(text: string): string {
  return escapeHtml(text)
    .replace(/`([^`]+)`/g, "<code>$1</code>")
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/\[([^\]]+)\]\((https?:\/\/[^)\s]+|\/[^)\s]*)\)/g, (_m, label: string, href: string) => {
      const external = href.startsWith("http");
      return `<a href="${href}"${external ? ' rel="noopener"' : ""}>${label}</a>`;
    });
}

export type Block =
  | { p: string }
  | { ul: string[] }
  | { ol: string[] }
  | { code: string }
  | { note: string }
  | { table: { head: string[]; rows: string[][] } };

export function slug(s: string): string {
  return s
    .toLowerCase()
    .replace(/[^a-z0-9Ѐ-ӿ]+/g, "-")
    .replace(/^-|-$/g, "");
}
