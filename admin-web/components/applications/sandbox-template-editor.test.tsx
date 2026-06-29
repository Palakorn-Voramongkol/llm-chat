import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { SandboxTemplateEditor } from "./sandbox-template-editor";
import { api } from "@/lib/api";

vi.mock("@/lib/api", () => ({
  api: { getSandboxTemplate: vi.fn(), saveSandboxTemplate: vi.fn() },
  ApiError: class extends Error {},
}));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

describe("SandboxTemplateEditor", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (api.getSandboxTemplate as ReturnType<typeof vi.fn>).mockResolvedValue({
      configured: true, ok: true, appCode: "kabytech", version: 2,
      template: [{ path: "README.md", dir: false, content: "# hi" }],
      migrateInstructions: null, updatedAt: "t",
    });
    (api.saveSandboxTemplate as ReturnType<typeof vi.fn>).mockResolvedValue({ ok: true, version: 2, updatedAt: "t2" });
  });

  it("loads and shows the version + a tree node", async () => {
    render(<SandboxTemplateEditor pid="p1" appId="a1" />);
    await waitFor(() => expect(screen.getByText(/v2/)).toBeInTheDocument());
    expect(screen.getByText("README.md")).toBeInTheDocument();
  });

  it("saves a content edit without publishing", async () => {
    render(<SandboxTemplateEditor pid="p1" appId="a1" />);
    await screen.findByText("README.md");
    fireEvent.click(screen.getByRole("button", { name: /^save$/i }));
    await waitFor(() => expect(api.saveSandboxTemplate).toHaveBeenCalledWith("p1", "a1",
      expect.objectContaining({ publish: false })));
  });
});
