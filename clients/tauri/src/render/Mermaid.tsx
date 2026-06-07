import { useEffect, useRef, useState } from "react";
import mermaid from "mermaid";
import { Fallback } from "./Fallback";

mermaid.initialize({ startOnLoad: false, theme: "dark", securityLevel: "strict" });

let counter = 0;

/** Render a ```mermaid fenced block to an SVG diagram. */
export function Mermaid({ code }: { code: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const id = `lumina-mermaid-${counter++}`;
    mermaid
      .render(id, code)
      .then(({ svg }) => {
        if (!cancelled && ref.current) ref.current.innerHTML = svg;
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [code]);

  if (error) return <Fallback title="Couldn't render this Mermaid diagram" source={code} />;
  return <div ref={ref} className="my-3 flex justify-center overflow-x-auto" />;
}
