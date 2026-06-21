import { describe, it, expect } from "vitest";
import { fmtCount, fmtBytes } from "@/components/users/columns";

describe("usage formatting", () => {
  it("formats thousands and dashes missing", () => {
    expect(fmtCount(123456)).toBe("123,456");
    expect(fmtCount(undefined)).toBe("—");
    expect(fmtCount(0)).toBe("0");
  });

  it("fmtBytes formats B/KB/MB and dashes missing", () => {
    expect(fmtBytes(undefined)).toBe("—");
    expect(fmtBytes(null)).toBe("—");
    expect(fmtBytes(512)).toBe("512 B");
    expect(fmtBytes(1536)).toBe("1.5 KB");
    expect(fmtBytes(3 * 1024 * 1024 + 512 * 1024)).toBe("3.5 MB");
  });
});
