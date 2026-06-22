import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { AuthCard } from "./Card";

describe("AuthCard (split-screen shell)", () => {
  it("renders the KabyTech brand, headline, title, and children", () => {
    render(<AuthCard title="Sign in" subtitle="Welcome back."><button>Submit</button></AuthCard>);
    // the "K" logo mark appears in both the desktop panel and the mobile band
    expect(screen.getAllByText("K").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Kaby").length).toBeGreaterThan(0);
    expect(screen.getByText(/freight document intelligence/i)).toBeInTheDocument();
    expect(screen.getByText("Sign in")).toBeInTheDocument();
    expect(screen.getByText("Welcome back.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Submit" })).toBeInTheDocument();
  });
});
