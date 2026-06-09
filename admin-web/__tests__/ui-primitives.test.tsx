import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { Card, CardHeader, CardTitle, CardContent } from "../components/ui/card";
import { Switch } from "../components/ui/switch";
import { Checkbox } from "../components/ui/checkbox";

describe("Card primitive", () => {
  it("renders title and content", () => {
    render(
      <Card>
        <CardHeader><CardTitle>Total users</CardTitle></CardHeader>
        <CardContent>24</CardContent>
      </Card>,
    );
    expect(screen.getByText("Total users")).toBeInTheDocument();
    expect(screen.getByText("24")).toBeInTheDocument();
  });
});

describe("Switch primitive", () => {
  it("renders an unchecked switch role", () => {
    render(<Switch aria-label="Skip MFA prompt" />);
    const s = screen.getByRole("switch", { name: "Skip MFA prompt" });
    expect(s).toBeInTheDocument();
    expect(s).toHaveAttribute("aria-checked", "false");
  });
});

describe("Checkbox primitive", () => {
  it("renders an unchecked checkbox role", () => {
    render(<Checkbox aria-label="chat.user" />);
    const c = screen.getByRole("checkbox", { name: "chat.user" });
    expect(c).toBeInTheDocument();
    expect(c).toHaveAttribute("aria-checked", "false");
  });
});
