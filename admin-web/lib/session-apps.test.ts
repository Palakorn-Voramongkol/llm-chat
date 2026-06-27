import { describe, it, expect } from "vitest";
import { chatSessionsUrl } from "./session-apps";

describe("chatSessionsUrl", () => {
  it("appends ?app= when an app is selected", () => {
    expect(chatSessionsUrl("llm-chat")).toBe("/api/chat-sessions?app=llm-chat");
  });
  it("url-encodes the app key", () => {
    expect(chatSessionsUrl("app two")).toBe("/api/chat-sessions?app=app%20two");
  });
  it("omits the param when no app is selected", () => {
    expect(chatSessionsUrl("")).toBe("/api/chat-sessions");
  });
});
