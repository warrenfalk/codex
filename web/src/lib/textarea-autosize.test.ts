import { afterEach, describe, expect, it, vi } from "vitest";

import {
  getTextareaAutosizeResult,
  syncTextareaHeight,
} from "./textarea-autosize";

const originalScrollHeightDescriptor = Object.getOwnPropertyDescriptor(
  HTMLTextAreaElement.prototype,
  "scrollHeight",
);

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
  if (originalScrollHeightDescriptor) {
    Object.defineProperty(
      HTMLTextAreaElement.prototype,
      "scrollHeight",
      originalScrollHeightDescriptor,
    );
  } else {
    Reflect.deleteProperty(HTMLTextAreaElement.prototype, "scrollHeight");
  }
});

describe("getTextareaAutosizeResult", () => {
  it("keeps the same height when the content height already fits", () => {
    expect(
      getTextareaAutosizeResult({
        maxHeightPx: 320,
        measuredContentHeightPx: 88,
        minHeightPx: 40,
      }),
    ).toEqual({
      nextHeightPx: 88,
      overflowY: "hidden",
    });
  });

  it("respects the minimum height when content is shorter", () => {
    expect(
      getTextareaAutosizeResult({
        maxHeightPx: 320,
        measuredContentHeightPx: 18,
        minHeightPx: 40,
      }),
    ).toEqual({
      nextHeightPx: 40,
      overflowY: "hidden",
    });
  });

  it("caps the height and enables scrolling when content exceeds the max height", () => {
    expect(
      getTextareaAutosizeResult({
        maxHeightPx: 160,
        measuredContentHeightPx: 240,
        minHeightPx: 40,
      }),
    ).toEqual({
      nextHeightPx: 160,
      overflowY: "auto",
    });
  });
});

describe("syncTextareaHeight", () => {
  it("shrinks after content is deleted", () => {
    vi.spyOn(
      HTMLTextAreaElement.prototype,
      "getBoundingClientRect",
    ).mockImplementation(function mockGetBoundingClientRect(
      this: HTMLTextAreaElement,
    ) {
      const height = Number.parseFloat(this.style.height) || 0;
      return {
        bottom: height,
        height,
        left: 0,
        right: 320,
        toJSON: () => ({}),
        top: 0,
        width: 320,
        x: 0,
        y: 0,
      };
    });
    Object.defineProperty(HTMLTextAreaElement.prototype, "scrollHeight", {
      configurable: true,
      get() {
        const lineCount = Math.max(1, this.value.split("\n").length);
        const naturalHeightPx = lineCount * 20;
        const currentHeightPx = Number.parseFloat(this.style.height) || 0;
        return Math.max(naturalHeightPx, currentHeightPx);
      },
    });

    const input = document.createElement("textarea");
    input.style.borderBottomWidth = "0px";
    input.style.borderTopWidth = "0px";
    input.style.maxHeight = "120px";
    input.style.minHeight = "20px";
    document.body.appendChild(input);

    input.value = "line 1\nline 2";
    syncTextareaHeight(input);
    expect(input.style.height).toBe("40px");

    input.value = "line 1";
    syncTextareaHeight(input);
    expect(input.style.height).toBe("20px");
    expect(input.style.overflowY).toBe("hidden");
  });
});
