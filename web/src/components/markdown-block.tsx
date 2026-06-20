import type { ComponentProps, MouseEvent } from "react";
import {
  defaultUrlTransform,
  Streamdown,
  type Components,
  type StreamdownProps,
  type UrlTransform,
} from "streamdown";

import { sourceFileRouteFromHref } from "@/lib/source-file-links";

const markdownLinkSafety: NonNullable<StreamdownProps["linkSafety"]> = {
  enabled: false,
};

export type SourceFileLinkOptions = {
  onNavigate?: (route: string) => void;
  root?: string | null;
  threadId: string;
};

type Props = {
  className?: string;
  sourceFileLinks?: SourceFileLinkOptions;
  text: string;
};

function routeFromRenderedHref(href: string | undefined): string | null {
  if (!href) {
    return null;
  }

  if (href.startsWith("/files?")) {
    return href;
  }

  try {
    const url = new URL(href, window.location.origin);
    return url.origin === window.location.origin && url.pathname === "/files"
      ? `${url.pathname}${url.search}`
      : null;
  } catch {
    return null;
  }
}

function rewriteSourceFileMarkdownLinks(
  text: string,
  sourceFileLinks: SourceFileLinkOptions | undefined,
): string {
  if (!sourceFileLinks) {
    return text;
  }

  return text.replace(
    /(\[[^\]\n]+\]\()(<[^>\n]+>|[^)\s\n]+)((?:\s+["'][^"'\n]*["'])?\))/gu,
    (match, prefix: string, rawDestination: string, suffix: string) => {
      const destination = rawDestination.startsWith("<")
        ? rawDestination.slice(1, -1)
        : rawDestination;
      const route = sourceFileRouteFromHref(destination, sourceFileLinks);
      return route ? `${prefix}<${route}>${suffix}` : match;
    },
  );
}

function shouldPush(event: MouseEvent<HTMLAnchorElement>): boolean {
  return (
    event.button === 0 &&
    !event.defaultPrevented &&
    !event.metaKey &&
    !event.altKey &&
    !event.ctrlKey &&
    !event.shiftKey
  );
}

function buildComponents(
  sourceFileLinks: SourceFileLinkOptions | undefined,
): Components | undefined {
  if (!sourceFileLinks) {
    return undefined;
  }

  return {
    a({
      children,
      href,
      node: _node,
      onClick,
      ...props
    }: ComponentProps<"a"> & { node?: unknown }) {
      const route =
        routeFromRenderedHref(href) ??
        sourceFileRouteFromHref(href, sourceFileLinks);
      if (!route) {
        return (
          <a href={href} onClick={onClick} {...props}>
            {children}
          </a>
        );
      }

      return (
        <a
          href={route}
          onClick={(event) => {
            onClick?.(event);
            if (!shouldPush(event)) {
              return;
            }
            event.preventDefault();
            sourceFileLinks.onNavigate?.(route);
          }}
          {...props}
        >
          {children}
        </a>
      );
    },
  };
}

function buildUrlTransform(
  sourceFileLinks: SourceFileLinkOptions | undefined,
): UrlTransform | undefined {
  if (!sourceFileLinks) {
    return undefined;
  }

  return (url, key, node) =>
    sourceFileRouteFromHref(url, sourceFileLinks) ??
    defaultUrlTransform(url, key, node);
}

export function MarkdownBlock({
  className = "message-markdown",
  sourceFileLinks,
  text,
}: Props) {
  const renderedText = rewriteSourceFileMarkdownLinks(text, sourceFileLinks);

  return (
    <Streamdown
      className={className}
      components={buildComponents(sourceFileLinks)}
      dir="auto"
      linkSafety={markdownLinkSafety}
      urlTransform={buildUrlTransform(sourceFileLinks)}
    >
      {renderedText}
    </Streamdown>
  );
}
