import { describe, it, expect } from "vitest";
import { fmtTokens, fmtCost } from "@/components/users/columns";

describe("token usage formatting", () => {
  it("formats thousands and dashes missing", () => {
    expect(fmtTokens(123456)).toBe("123,456");
    expect(fmtTokens(undefined)).toBe("—");
    expect(fmtTokens(0)).toBe("0");
  });

  it("fmtCost formats dollars", () => {
    expect(fmtCost(undefined)).toBe("—");
    expect(fmtCost(0)).toBe("$0.0000");
    expect(fmtCost(0.5)).toBe("$0.5000");
    expect(fmtCost(1.5)).toBe("$1.50");
    expect(fmtCost(10.123)).toBe("$10.12");
  });
});
