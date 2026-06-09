import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { Card, CardHeader, CardTitle, CardContent } from "../components/ui/card";

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
