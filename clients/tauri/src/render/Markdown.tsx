import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";
import rehypeRaw from "rehype-raw";
import rehypeSanitize, { defaultSchema } from "rehype-sanitize";
import { CodeBlock } from "./CodeBlock";
import { Mermaid } from "./Mermaid";
import { PlantUml } from "./PlantUml";
import { Pdf } from "./Pdf";
import { api } from "../lib/tauri";

// Sanitize runs LAST (on claude's raw HTML + the katex/highlight output we
// trust). Allow className/style/aria-hidden so KaTeX (output:"html") survives;
// the default schema still strips <script>, event handlers, etc.
const baseStar = (defaultSchema.attributes && defaultSchema.attributes["*"]) || [];
const schema = {
  ...defaultSchema,
  attributes: {
    ...defaultSchema.attributes,
    "*": [...baseStar, "className", "style", "ariaHidden"],
  },
};

function textOf(node: unknown): string {
  if (node == null) return "";
  if (typeof node === "string") return node;
  if (Array.isArray(node)) return node.map(textOf).join("");
  if (typeof node === "object" && "props" in (node as any)) {
    return textOf((node as any).props.children);
  }
  return "";
}

/** Render claude's markdown answer: prose + GFM tables, KaTeX math, sanitized
 *  raw HTML, inline images, and fenced blocks dispatched to code/mermaid/
 *  plantuml/pdf renderers. */
export function Markdown({ content, plantumlServer }: { content: string; plantumlServer: string }) {
  const components: Components = {
    // Block code is wrapped in <pre><code>; dispatch by language.
    pre(props: any) {
      const el: any = Array.isArray(props.children) ? props.children[0] : props.children;
      const className: string = el?.props?.className ?? "";
      const lang = /language-([\w-]+)/.exec(className)?.[1] ?? "";
      const raw = textOf(el?.props?.children).replace(/\n$/, "");
      if (lang === "mermaid") return <Mermaid code={raw} />;
      if (lang === "plantuml" || lang === "puml") return <PlantUml code={raw} server={plantumlServer} />;
      if (lang === "pdf") return <Pdf url={raw.trim()} />;
      return <CodeBlock code={raw} lang={lang} />;
    },
    a(props: any) {
      const url: string = props.href ?? "";
      if (url.toLowerCase().endsWith(".pdf")) return <Pdf url={url} />;
      return (
        <a
          href={url}
          onClick={(e) => {
            e.preventDefault();
            if (url) void api.openExternal(url);
          }}
        >
          {props.children}
        </a>
      );
    },
    img(props: any) {
      return (
        <img
          src={props.src}
          alt={props.alt ?? ""}
          className="my-2 max-w-full rounded-lg border border-slate-200 dark:border-slate-800"
        />
      );
    },
  };

  return (
    <div className="lumina-prose">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[rehypeRaw, [rehypeKatex, { output: "html" }], [rehypeSanitize, schema]]}
        components={components}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}
