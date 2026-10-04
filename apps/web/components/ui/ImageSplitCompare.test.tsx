import { describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToString } from "react-dom/server";
import { ImageSplitCompare } from "./ImageSplitCompare";

describe("ImageSplitCompare", () => {
  it("renders with slider role and accessible values", () => {
    const html = renderToString(
      createElement(ImageSplitCompare, {
        beforeSrc: "/sample-a.png",
        afterSrc: "/sample-b.png",
        beforeLabel: "A: Época 0",
        afterLabel: "B: Época 10",
        initialPosition: 50,
      })
    );

    expect(html).toContain('role="slider"');
    expect(html).toContain('aria-valuenow="50"');
    expect(html).toContain('aria-valuemin="0"');
    expect(html).toContain('aria-valuemax="100"');
    expect(html).toContain("A: Época 0");
    expect(html).toContain("B: Época 10");
    expect(html).toContain('src="/sample-a.png"');
    expect(html).toContain('src="/sample-b.png"');
  });

  it("supports vertical orientation", () => {
    const html = renderToString(
      createElement(ImageSplitCompare, {
        beforeSrc: "/sample-a.png",
        afterSrc: "/sample-b.png",
        orientation: "vertical",
        initialPosition: 40,
      })
    );

    expect(html).toContain('aria-valuenow="40"');
    expect(html).toContain("cursor-ns-resize");
  });
});
