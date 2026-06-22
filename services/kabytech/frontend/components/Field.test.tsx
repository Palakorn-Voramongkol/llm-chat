import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { Field, isEmail, passwordStrength } from "./Field";

const required = (s: string) => (!s ? "required" : undefined);

describe("Field + isEmail", () => {
  it("isEmail accepts valid addresses and rejects invalid", () => {
    expect(isEmail("a@b.co")).toBe(true);
    expect(isEmail("user@kabytech.local")).toBe(true);
    expect(isEmail("nope")).toBe(false);
    expect(isEmail("a@b")).toBe(false);
    expect(isEmail("")).toBe(false);
  });

  it("shows no error before the field is touched", () => {
    render(<Field placeholder="X" value="" onChange={() => {}} validate={required} />);
    expect(screen.queryByText("required")).toBeNull();
    expect(screen.getByPlaceholderText("X")).not.toHaveAttribute("aria-invalid");
  });

  it("validates live once touched (blur)", () => {
    render(<Field placeholder="X" value="" onChange={() => {}} validate={required} />);
    fireEvent.blur(screen.getByPlaceholderText("X"));
    expect(screen.getByText("required")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("X")).toHaveAttribute("aria-invalid", "true");
  });

  it("validates live on first keystroke", () => {
    // value stays "" (controlled, noop onChange) but the change event marks it
    // touched, so the required error appears live.
    render(<Field placeholder="X" value="" onChange={() => {}} validate={required} />);
    fireEvent.change(screen.getByPlaceholderText("X"), { target: { value: "a" } });
    expect(screen.getByText("required")).toBeInTheDocument();
  });

  it("submitted reveals the error without any interaction", () => {
    render(<Field placeholder="X" value="" onChange={() => {}} validate={required} submitted />);
    expect(screen.getByText("required")).toBeInTheDocument();
  });

  it("passwordStrength scales from weak to strong", () => {
    expect(passwordStrength("abc").score).toBeLessThanOrEqual(1);
    expect(passwordStrength("abc").label).toMatch(/weak/i);
    // long + upper/lower + digit + symbol -> strong (capped at 4)
    expect(passwordStrength("Abcdef12!xyz").score).toBe(4);
    expect(passwordStrength("Abcdef12!xyz").label).toBe("Strong");
  });
});
