/** Embed a PDF (a `.pdf` link or a ```pdf fence containing a URL). WebView2 /
 *  WebKit render PDFs natively in an iframe. */
export function Pdf({ url }: { url: string }) {
  return (
    <div className="my-3 overflow-hidden rounded-lg border border-slate-200 dark:border-slate-800">
      <div className="flex items-center justify-between bg-slate-100 px-3 py-1 text-xs text-slate-500 dark:bg-slate-900">
        <span>PDF</span>
        <a href={url} target="_blank" rel="noreferrer" className="text-brand-500 hover:underline">
          open ↗
        </a>
      </div>
      <iframe src={url} title="PDF document" className="h-[560px] w-full bg-white" />
    </div>
  );
}
