import { describe, it, expect } from "vitest";
import { entriesToTree, treeToEntries, isValidPath, type TemplateEntry } from "./sandbox-template";

describe("sandbox-template tree", () => {
  it("round-trips entries through the tree (files imply parent folders)", () => {
    const entries: TemplateEntry[] = [
      { path: "README.md", dir: false, content: "# hi" },
      { path: "notes/todo.md", dir: false, content: "do" },
      { path: "empty", dir: true, content: "" },
    ];
    const tree = entriesToTree(entries);
    const back = treeToEntries(tree);
    // README.md + notes/todo.md + empty/ survive; intermediate "notes" is NOT
    // emitted as a dir entry (it is implied by notes/todo.md).
    expect(back).toContainEqual({ path: "README.md", dir: false, content: "# hi" });
    expect(back).toContainEqual({ path: "notes/todo.md", dir: false, content: "do" });
    expect(back).toContainEqual({ path: "empty", dir: true, content: "" });
    expect(back.find((e) => e.path === "notes" && e.dir)).toBeUndefined();
  });

  it("validates paths the way the server confines them", () => {
    expect(isValidPath("a/b.txt")).toBe(true);
    expect(isValidPath("../x")).toBe(false);
    expect(isValidPath("/abs")).toBe(false);
    expect(isValidPath("a\\b")).toBe(false);
    expect(isValidPath("C:")).toBe(false);
    expect(isValidPath("a/./b")).toBe(false);
    expect(isValidPath("")).toBe(false);
  });
});
