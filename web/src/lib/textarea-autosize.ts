const TEXTAREA_MIRROR_ID = "textarea-autosize-mirror";

type TextareaAutosizeInput = {
  maxHeightPx: number;
  measuredContentHeightPx: number;
  minHeightPx: number;
};

type TextareaAutosizeResult = {
  nextHeightPx: number;
  overflowY: "auto" | "hidden";
};

function parseLengthPx(value: string, viewportHeightPx: number): number {
  const trimmed = value.trim();

  if (trimmed === "" || trimmed === "none") {
    return Number.POSITIVE_INFINITY;
  }

  if (trimmed.endsWith("px")) {
    return Number.parseFloat(trimmed);
  }

  if (trimmed.endsWith("vh")) {
    return (viewportHeightPx * Number.parseFloat(trimmed)) / 100;
  }

  const numeric = Number.parseFloat(trimmed);
  return Number.isFinite(numeric) ? numeric : Number.POSITIVE_INFINITY;
}

function getTextareaMirror(): HTMLTextAreaElement {
  const existingMirror = document.getElementById(TEXTAREA_MIRROR_ID);
  if (existingMirror instanceof HTMLTextAreaElement) {
    return existingMirror;
  }

  const mirror = document.createElement("textarea");
  mirror.id = TEXTAREA_MIRROR_ID;
  mirror.setAttribute("aria-hidden", "true");
  mirror.setAttribute("tabindex", "-1");
  document.body.appendChild(mirror);
  return mirror;
}

function syncMirrorStyles(
  mirror: HTMLTextAreaElement,
  input: HTMLTextAreaElement,
  computedStyle: CSSStyleDeclaration,
): void {
  for (let index = 0; index < computedStyle.length; index += 1) {
    const propertyName = computedStyle.item(index);
    mirror.style.setProperty(
      propertyName,
      computedStyle.getPropertyValue(propertyName),
    );
  }

  const inputWidthPx = input.getBoundingClientRect().width;
  mirror.style.height = "0px";
  mirror.style.left = "-9999px";
  mirror.style.maxHeight = "none";
  mirror.style.minHeight = "0px";
  mirror.style.overflow = "hidden";
  mirror.style.pointerEvents = "none";
  mirror.style.position = "absolute";
  mirror.style.top = "0px";
  mirror.style.visibility = "hidden";
  mirror.style.width =
    inputWidthPx > 0 ? `${inputWidthPx}px` : computedStyle.width;
}

function measureTextareaContentHeight(
  input: HTMLTextAreaElement,
  computedStyle: CSSStyleDeclaration,
  borderHeightPx: number,
): number {
  const mirror = getTextareaMirror();
  syncMirrorStyles(mirror, input, computedStyle);
  mirror.value = input.value.endsWith("\n") ? `${input.value} ` : input.value;
  return mirror.scrollHeight + borderHeightPx;
}

export function getTextareaAutosizeResult({
  maxHeightPx,
  measuredContentHeightPx,
  minHeightPx,
}: TextareaAutosizeInput): TextareaAutosizeResult {
  return {
    nextHeightPx: Math.min(
      Math.max(measuredContentHeightPx, minHeightPx),
      maxHeightPx,
    ),
    overflowY: measuredContentHeightPx > maxHeightPx ? "auto" : "hidden",
  };
}

export function syncTextareaHeight(input: HTMLTextAreaElement): void {
  const computedStyle = window.getComputedStyle(input);
  const borderHeightPx =
    Number.parseFloat(computedStyle.borderTopWidth) +
    Number.parseFloat(computedStyle.borderBottomWidth);
  const maxHeightPx = parseLengthPx(
    computedStyle.maxHeight,
    window.innerHeight,
  );
  const minHeightPx = parseLengthPx(
    computedStyle.minHeight,
    window.innerHeight,
  );
  const result = getTextareaAutosizeResult({
    maxHeightPx,
    measuredContentHeightPx: measureTextareaContentHeight(
      input,
      computedStyle,
      borderHeightPx,
    ),
    minHeightPx,
  });

  input.style.height = `${result.nextHeightPx}px`;
  input.style.overflowY = result.overflowY;
}
