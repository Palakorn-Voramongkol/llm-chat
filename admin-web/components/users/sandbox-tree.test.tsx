import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { SandboxTree } from "./sandbox-tree";
import { buildTree } from "@/lib/sandbox-tree";

describe("SandboxTree", () => {
  it("renders folders, files and sizes (top level expanded)", () => {
    const nodes = buildTree([
      { path: "projects", dir: true, size: 0 },
      { path: "projects/main.rs", dir: false, size: 842 },
      { path: "todo.md", dir: false, size: 310 },
    ]);
    render(<SandboxTree nodes={nodes} />);
    expect(screen.getByText("projects")).toBeInTheDocument();
    expect(screen.getByText("todo.md")).toBeInTheDocument();
    expect(screen.getByText("main.rs")).toBeInTheDocument(); // top-level folder expanded
    expect(screen.getByText("842 B")).toBeInTheDocument();
    expect(screen.getByText("310 B")).toBeInTheDocument();
  });
});
