import { useState } from "react";
import { SendHorizontal } from "lucide-react";

export function Composer({ onSend, disabled }: { onSend: (t: string) => void; disabled?: boolean }) {
  const [text, setText] = useState("");
  const submit = () => {
    if (text.trim()) {
      onSend(text);
      setText("");
    }
  };
  return (
    <div className="border-t border-slate-200 bg-white/70 p-3 backdrop-blur dark:border-slate-800 dark:bg-slate-900/60">
      <div className="mx-auto flex max-w-3xl items-end gap-2">
        <textarea
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              submit();
            }
          }}
          rows={1}
          placeholder="Ask anything…  (Enter to send, Shift+Enter for a new line)"
          className="max-h-40 flex-1 resize-none rounded-xl border border-slate-300 bg-white px-3 py-2.5 text-sm outline-none transition focus:border-brand-500 dark:border-slate-700 dark:bg-slate-950"
        />
        <button
          onClick={submit}
          disabled={disabled || !text.trim()}
          className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-brand-600 text-white transition hover:bg-brand-500 disabled:cursor-not-allowed disabled:opacity-50"
        >
          <SendHorizontal size={18} />
        </button>
      </div>
    </div>
  );
}
