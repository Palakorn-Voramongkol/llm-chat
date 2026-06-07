import { useCallback, useEffect, useRef, useState } from "react";
import { api, onEvent } from "../lib/tauri";

export interface ChatMessage {
  id: string;
  role: "user" | "assistant";
  text: string;
  pending?: boolean;
  error?: boolean;
  latencyMs?: number | null;
}

interface Frame {
  type?: string;
  id?: string;
  text?: string;
  latencyMs?: number | null;
}

export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [connected, setConnected] = useState(false);
  const [thinking, setThinking] = useState(false);
  const [connError, setConnError] = useState<string | null>(null);
  const counter = useRef(0);

  useEffect(() => {
    let unframe: (() => void) | undefined;
    let unclosed: (() => void) | undefined;
    (async () => {
      unframe = await onEvent<Frame>("chat://frame", (frame) => {
        if (frame.type === "a") {
          setMessages((m) =>
            m.map((msg) =>
              msg.id === frame.id
                ? { ...msg, text: frame.text ?? "", pending: false, latencyMs: frame.latencyMs }
                : msg,
            ),
          );
          setThinking(false);
        } else if (frame.type === "err") {
          setMessages((m) => {
            const i = [...m].reverse().findIndex((x) => x.role === "assistant" && x.pending);
            if (i === -1) return m;
            const real = m.length - 1 - i;
            const copy = [...m];
            copy[real] = { ...copy[real], text: frame.text ?? "error", pending: false, error: true };
            return copy;
          });
          setThinking(false);
        }
        // initialized / ack → ignore
      });
      unclosed = await onEvent("chat://closed", () => setConnected(false));
      try {
        await api.chatConnect();
        setConnected(true);
      } catch (e) {
        setConnError(String(e));
      }
    })();
    return () => {
      unframe?.();
      unclosed?.();
      void api.chatClose();
    };
  }, []);

  const send = useCallback(async (text: string) => {
    const t = text.trim();
    if (!t) return;
    counter.current += 1;
    const id = `m${counter.current}`;
    setMessages((m) => [
      ...m,
      { id: `u-${id}`, role: "user", text: t },
      { id, role: "assistant", text: "", pending: true },
    ]);
    setThinking(true);
    try {
      await api.chatSend(id, t);
    } catch (e) {
      setMessages((m) =>
        m.map((msg) => (msg.id === id ? { ...msg, text: String(e), pending: false, error: true } : msg)),
      );
      setThinking(false);
    }
  }, []);

  return { messages, connected, thinking, connError, send };
}
