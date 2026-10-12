import { describe, expect, it } from "vitest";
import { shapeFor } from "./shape";

describe("island shapes", () => {
  it("grows from pill to panel", () => {
    expect(shapeFor("pill", false).width).toBe(140);
    expect(shapeFor("panel", false).width).toBe(420);
  });

  it("uses the wide shape for the Computer Use banner", () => {
    expect(shapeFor("pill", true).width).toBe(420);
    expect(shapeFor("panel", true).width).toBe(420);
  });
});
