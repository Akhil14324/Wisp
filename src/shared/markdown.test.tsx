import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Markdown, parseBlocks } from "./markdown";

describe("markdown", () => {
  it("parses blocks", () => {
    const b = parseBlocks("# Total\n\nThe sum is **42**.\n\n- a\n- b\n\n```\nx = 1\n```");
    expect(b.map((x) => x.kind)).toEqual(["h", "p", "ul", "code"]);
  });

  it("renders inline formatting as elements, never HTML", () => {
    const { container } = render(<Markdown text={'Use `npm` and **bold** <img src=x onerror="alert(1)">'} />);
    expect(container.querySelector("code")?.textContent).toBe("npm");
    expect(container.querySelector("strong")?.textContent).toBe("bold");
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toContain("<img");
  });

  it("shows links as text", () => {
    const { container } = render(<Markdown text="See [docs](https://example.com)." />);
    expect(container.querySelector("a")).toBeNull();
    expect(container.textContent).toBe("See docs.");
  });
});
