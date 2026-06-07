import { Fallback } from "./Fallback";
import { safeEmbedUrl } from "../lib/url";
import { api } from "../lib/tauri";

/** Embed a PDF (a `.pdf` link or a ```pdf fence with a URL). Only http(s) URLs
 *  are embedded; the iframe is sandboxed so even a hostile PDF viewer can't run
 *  scripts. */
export function Pdf({ url }: { url: string }) {
  const safe = safeEmbedUrl(url);
  if (!safe) return <Fallback title="PDF URL must be http(s)" source={url} />;
  return (
    <div className="my-3 overflow-hidden rounded-lg border border-slate-200 dark:border-slate-800">
      <div className="flex items-center justify-between bg-slate-100 px-3 py-1 text-xs text-slate-500 dark:bg-slate-900">
        <span>PDF</span>
        <a
          href={safe}
          onClick={(e) => {
            e.preventDefault();
            void api.openExternal(safe);
          }}
          className="text-brand-500 hover:underline"
        >
          open ↗
        </a>
      </div>
      <iframe
        src={safe}
        title="PDF document"
        sandbox=""
        referrerPolicy="no-referrer"
        className="h-[560px] w-full bg-white"
      />
    </div>
  );
}
