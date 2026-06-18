"use client";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { cn } from "@/lib/utils";

/** Small inline markdown renderer for short rich text (tooltips, hints).
 * react-markdown sanitizes by default (no raw HTML). Element styling is theme-
 * safe so it reads on both the light and the (inverted) dark tooltip surface. */
export function Markdown({ children, className }: { children: string; className?: string }) {
  return (
    <div
      className={cn(
        "[&_p]:m-0 [&_p:not(:first-child)]:mt-1.5",
        "[&_strong]:font-semibold",
        "[&_em]:italic",
        "[&_code]:rounded [&_code]:bg-zinc-500/35 [&_code]:px-1 [&_code]:py-px [&_code]:font-mono [&_code]:text-[0.85em]",
        "[&_a]:underline [&_a]:underline-offset-2",
        "[&_ul]:my-1 [&_ul]:list-disc [&_ul]:pl-4 [&_ol]:my-1 [&_ol]:list-decimal [&_ol]:pl-4 [&_li]:mt-0.5",
        "[&_h1]:font-semibold [&_h2]:font-semibold [&_h3]:font-semibold",
        className,
      )}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{children}</ReactMarkdown>
    </div>
  );
}
