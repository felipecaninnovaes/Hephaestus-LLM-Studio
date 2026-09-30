import { describe, expect, it } from "bun:test";
import { type ListboxItem } from "./useListboxNavigation";

describe("Listbox Navigation Key Logic", () => {
  interface TestOption extends ListboxItem {
    id: string;
    label: string;
  }

  const items: TestOption[] = [
    { id: "1", label: "Option 1" },
    { id: "2", label: "Option 2", disabled: true },
    { id: "3", label: "Option 3" },
    { id: "4", label: "Option 4" },
    { id: "5", label: "Option 5", disabled: true },
  ];

  it("finds next non-disabled index cyclically", () => {
    function getNextIndex(current: number, list: TestOption[]): number {
      let next = (current + 1) % list.length;
      let attempts = 0;
      while (list[next]?.disabled && attempts < list.length) {
        next = (next + 1) % list.length;
        attempts++;
      }
      return next;
    }

    expect(getNextIndex(0, items)).toBe(2); // Skips index 1 (disabled)
    expect(getNextIndex(2, items)).toBe(3);
    expect(getNextIndex(3, items)).toBe(0); // Skips index 4 (disabled) and wraps to 0
  });

  it("finds previous non-disabled index cyclically", () => {
    function getPrevIndex(current: number, list: TestOption[]): number {
      let prev = (current - 1 + list.length) % list.length;
      let attempts = 0;
      while (list[prev]?.disabled && attempts < list.length) {
        prev = (prev - 1 + list.length) % list.length;
        attempts++;
      }
      return prev;
    }

    expect(getPrevIndex(2, items)).toBe(0); // Skips index 1 (disabled)
    expect(getPrevIndex(0, items)).toBe(3); // Skips index 4 (disabled) and wraps to 3
    expect(getPrevIndex(3, items)).toBe(2);
  });

  it("finds first and last enabled items for Home and End", () => {
    const firstEnabled = items.findIndex((item) => !item.disabled);
    expect(firstEnabled).toBe(0);

    let lastEnabled = -1;
    for (let i = items.length - 1; i >= 0; i--) {
      if (!items[i]?.disabled) {
        lastEnabled = i;
        break;
      }
    }
    expect(lastEnabled).toBe(3);
  });
});
