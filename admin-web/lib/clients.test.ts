import { describe, it, expect } from "vitest";
import { clientsBase, clientPath, clientSecretPath } from "./clients";

describe("client endpoint builders", () => {
  it("builds the project-scoped base", () => {
    expect(clientsBase("370")).toBe("/api/projects/370/apps");
  });
  it("builds a single-client path", () => {
    expect(clientPath("370", "abc")).toBe("/api/projects/370/apps/abc");
  });
  it("builds a secret path", () => {
    expect(clientSecretPath("370", "abc")).toBe("/api/projects/370/apps/abc/secret");
  });
  it("URL-encodes the ids", () => {
    expect(clientsBase("a b")).toBe("/api/projects/a%20b/apps");
    expect(clientPath("p", "a/b")).toBe("/api/projects/p/apps/a%2Fb");
  });
});
