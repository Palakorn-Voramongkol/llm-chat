import { AlertTriangle } from "lucide-react";

/** Shown when a diagram/renderer fails — never blanks the message; shows the
 *  raw source so nothing is lost. */
export function Fallback({ title, source }: { title: string; source: string }) {
  return (
    <div className="my-3 rounded-lg border border-amber-300/60 bg-amber-50 p-3 text-sm dark:border-amber-900/60 dark:bg-amber-950/30">
      <div className="mb-1.5 flex items-center gap-1.5 text-amber-700 dark:text-amber-400">
        <AlertTriangle size={15} /> {title}
      </div>
      <pre className="overflow-x-auto whitespace-pre-wrap font-mono text-[12px] text-slate-600 dark:text-slate-400">
        {source}
      </pre>
    </div>
  );
}
