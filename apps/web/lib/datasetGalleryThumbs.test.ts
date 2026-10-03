import { describe, expect, it } from "bun:test";
import type { ImageItem } from "@/types/studio";

describe("Dataset Gallery ImageItem with thumbUrl", () => {
  it("allows thumbUrl in ImageItem and supports undefined or null fallback", () => {
    const itemWithThumb: ImageItem = {
      id: "img-1",
      filename: "test.jpg",
      objectKey: "datasets/ds-1/images/img-1/test.jpg",
      bytes: 1024,
      width: 800,
      height: 600,
      mediaType: "image/jpeg",
      split: "train",
      url: "/api/datasets/ds-1/images/img-1/data",
      thumbUrl: "/api/datasets/ds-1/images/img-1/thumb",
      createdAt: "2026-10-03T12:00:00Z",
    };

    expect(itemWithThumb.thumbUrl).toBe("/api/datasets/ds-1/images/img-1/thumb");

    const itemWithoutThumb: ImageItem = {
      id: "img-2",
      filename: "test2.jpg",
      objectKey: "datasets/ds-1/images/img-2/test2.jpg",
      bytes: 2048,
      width: 1024,
      height: 768,
      mediaType: "image/jpeg",
      split: "train",
      url: "/api/datasets/ds-1/images/img-2/data",
      createdAt: "2026-10-03T12:00:00Z",
    };

    expect(itemWithoutThumb.thumbUrl).toBeUndefined();
  });
});
