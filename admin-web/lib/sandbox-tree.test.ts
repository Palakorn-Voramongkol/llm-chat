import { describe, it, expect } from "vitest";
import { buildTree } from "./sandbox-tree";

describe("buildTree", () => {
  it("nests '/'-separated entries, folders before files, with sizes", () => {
    const tree = buildTree([
      { path: "todo.md", dir: false, size: 310 },
      { path: "projects", dir: true, size: 0 },
      { path: "projects/main.rs", dir: false, size: 842 },
      { path: "projects/sub", dir: true, size: 0 },
    ]);
    // folders before files at the top level
    expect(tree.map((n) => n.name)).toEqual(["projects", "todo.md"]);
    const projects = tree.find((n) => n.name === "projects")!;
    expect(projects.children.map((n) => n.name)).toEqual(["sub", "main.rs"]);
    expect(tree.find((n) => n.name === "todo.md")!.size).toBe(310);
    expect(projects.children.find((n) => n.name === "main.rs")!.size).toBe(842);
    expect(projects.path).toBe("projects");
    expect(projects.children.find((n) => n.name === "main.rs")!.path).toBe("projects/main.rs");
  });

  it("returns [] for no entries", () => {
    expect(buildTree([])).toEqual([]);
  });
});
