import { useState } from "react";
import hljs from "highlight.js";
import { Check, Copy } from "lucide-react";

/** A code block with highlight.js syntax colors, a language label, and copy. */
export function CodeBlock({ code, lang }: { code: string; lang: string }) {
  const [copied, setCopied] = useState(false);

  // highlight.js escapes its output, so dangerouslySetInnerHTML is safe here.
  const html =
    lang && hljs.getLanguage(lang)
      ? hljs.highlight(code, { language: lang }).value
      : hljs.highlightAuto(code).value;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  return (
    <div className="my-3 overflow-hidden rounded-lg border border-slate-200 dark:border-slate-800">
      <div className="flex items-center justify-between bg-slate-100 px-3 py-1 text-xs text-slate-500 dark:bg-slate-900">
        <span className="font-mono">{lang || "code"}</span>
        <button
          onClick={copy}
          className="flex items-center gap-1 rounded px-1.5 py-0.5 transition hover:bg-slate-200 dark:hover:bg-slate-800"
        >
          {copied ? (
            <>
              <Check size={13} /> Copied
            </>
          ) : (
            <>
              <Copy size={13} /> Copy
            </>
          )}
        </button>
      </div>
      <pre className="overflow-x-auto bg-slate-50 p-3 text-[13px] leading-relaxed dark:bg-[#0d1117]">
        <code className="hljs bg-transparent" dangerouslySetInnerHTML={{ __html: html }} />
      </pre>
    </div>
  );
}
