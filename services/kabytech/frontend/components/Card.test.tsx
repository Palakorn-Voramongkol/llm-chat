import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { AuthCard } from "./Card";

describe("AuthCard (split-screen shell)", () => {
  it("renders the brand, headline, title, and children", () => {
    render(<AuthCard title="Sign in" subtitle="Welcome back."><button>Submit</button></AuthCard>);
    // wordmark appears in both the desktop panel and the mobile band
    expect(screen.getAllByText(/kabytech/i).length).toBeGreaterThan(0);
    expect(screen.getByText(/your ai workspace, one login/i)).toBeInTheDocument();
    expect(screen.getByText("Sign in")).toBeInTheDocument();
    expect(screen.getByText("Welcome back.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Submit" })).toBeInTheDocument();
  });
});
