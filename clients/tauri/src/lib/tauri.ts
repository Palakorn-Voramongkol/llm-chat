// Typed wrappers over the Rust shell's IPC commands + event stream.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface Identity {
  sub: string;
  email: string | null;
  name: string | null;
  roles: string[];
}

export interface AppConfig {
  app_name: string;
  issuer: string;
  manager_ws: string;
  project: string | null;
  client_id: string | null;
  plantuml_server: string;
  required_role: string;
}

/** A settled answer forwarded from the manager's /chat stream. */
export interface AnswerFrame {
  id: string;
  seq: number;
  text: string;
  latencyMs?: number | null;
}

export const api = {
  getConfig: () => invoke<AppConfig>("get_config"),
  login: () => invoke<Identity>("login"),
  restore: () => invoke<Identity | null>("restore"),
  logout: () => invoke<void>("logout"),
  // chat (wired in the chat step)
  chatConnect: () => invoke<string>("chat_connect"),
  chatSend: (text: string) => invoke<void>("chat_send", { text }),
  chatClose: () => invoke<void>("chat_close"),
};

export function onEvent<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => cb(e.payload as T));
}
