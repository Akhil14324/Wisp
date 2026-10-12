import { describe, expect, it } from "vitest";
import { toPhysical } from "./geometry";

describe("overlay geometry", () => {
  it("scales CSS pixels by the device pixel ratio", () => {
    expect(toPhysical([[100, 50]], 1.5)).toEqual([[150, 75]]);
    expect(toPhysical([[10.33, 0]], 2)).toEqual([[20.7, 0]]);
    expect(toPhysical([], 1.25)).toEqual([]);
  });
});
