import { useState } from "react";
import { Check, Copy, Loader2, Sparkles } from "lucide-react";
import { Markdown } from "../render/Markdown";
import type { ChatMessage } from "./useChat";

// LINE talk-bubble tail ("horn"), traced from the pixels of the user's real
// LINE screenshot: an up-right pointed tip with a concave underside scoop.
// Drawn in a 28×22 viewBox; the left edge (x=14) overlaps the bubble's squared
// top corner so it merges seamlessly, the rest protrudes. Mirrored via CSS for
// received bubbles (see .lumina-tail-recv).
const TAIL_PATH =
  "M6,4 C11,3.5 14,3 18,2 C22,1 26,2 26.5,4 C25,6 20,7 16,11 C14.5,14 14,16 14,20 L14,4 L6,4 Z";

function fmtTime(t?: number): string {
  if (!t) return "";
  try {
    return new Date(t).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  } catch {
    return "";
  }
}
function fmtFull(t?: number): string {
  if (!t) return "";
  try {
    return new Date(t).toLocaleString();
  } catch {
    return "";
  }
}

export function Message({ msg, plantumlServer }: { msg: ChatMessage; plantumlServer: string }) {
  const isUser = msg.role === "user";
  const [copied, setCopied] = useState(false);
  const time = fmtTime(msg.time);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(msg.text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  };

  const widthCls = isUser ? "max-w-[78%]" : "max-w-[88%]";
  // LINE-style: green sent bubble (horn tail top-right), white/dark received
  // bubble (horn tail top-left, mirrored). The tail is an SVG horn anchored at
  // the corner (see styles.css + TAIL_PATH below); ml-1 on received keeps the
  // left-pointing horn clear of the avatar.
  const bubbleCls = isUser
    ? "lumina-bubble lumina-bubble-sent"
    : "lumina-bubble lumina-bubble-received ml-2 text-slate-900 dark:text-slate-100";

  const TimeLabel = () =>
    time ? (
      <span title={fmtFull(msg.time)} className="mb-1 shrink-0 select-none text-[11px] text-slate-400">
        {time}
      </span>
    ) : null;

  return (
    <div className={`flex items-end gap-2 ${isUser ? "justify-end" : "justify-start"}`}>
      {!isUser && (
        <div className="flex h-8 w-8 shrink-0 items-center justify-center self-start rounded-full bg-gradient-to-br from-brand-400 to-brand-600 text-white">
          <Sparkles size={15} />
        </div>
      )}
      {isUser && <TimeLabel />}

      <div
        className={`min-w-0 ${widthCls} px-3.5 py-2 ${bubbleCls} ${msg.error ? "ring-1 ring-red-400" : ""}`}
      >
        <span
          className={`lumina-tail ${isUser ? "lumina-tail-sent" : "lumina-tail-recv"}`}
          aria-hidden="true"
        >
          <svg viewBox="0 0 28 22">
            <path d={TAIL_PATH} />
          </svg>
        </span>
        {isUser ? (
          <p className="whitespace-pre-wrap break-words">{msg.text}</p>
        ) : msg.pending ? (
          <span className="flex items-center gap-2 text-slate-500">
            <Loader2 className="animate-spin" size={15} /> thinking…
          </span>
        ) : (
          <>
            <Markdown content={msg.text} plantumlServer={plantumlServer} />
            <div className="mt-1.5 flex items-center justify-end gap-2 border-t border-slate-100 pt-1.5 dark:border-slate-700/60">
              {!isUser && msg.latencyMs != null && msg.latencyMs >= 0 && (
                <span className="text-[11px] text-slate-400">{(msg.latencyMs / 1000).toFixed(1)}s</span>
              )}
              <button
                onClick={copy}
                title="Copy response"
                className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] text-slate-400 transition hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-slate-700 dark:hover:text-slate-200"
              >
                {copied ? (
                  <>
                    <Check size={12} /> Copied
                  </>
                ) : (
                  <>
                    <Copy size={12} /> Copy
                  </>
                )}
              </button>
            </div>
          </>
        )}
      </div>

      {!isUser && !msg.pending && <TimeLabel />}
    </div>
  );
}
