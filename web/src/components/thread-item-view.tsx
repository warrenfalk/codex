import type { ReactNode } from "react";

import type { ThreadItem } from "@/types/protocol";

import { MarkdownBlock, type SourceFileLinkOptions } from "./markdown-block";

type Props = {
  anchorId?: string;
  item: ThreadItem;
  onJumpToPreviousUserTurn?: () => void;
  onUseSuggestedReply?: (reply: string) => void;
  sourceFileLinks?: SourceFileLinkOptions;
  runtimeText?: string;
};

type FileChangeItem = Extract<ThreadItem, { type: "fileChange" }>;
type FileChange = FileChangeItem["changes"][number];
type FunctionCallOutputItem = Extract<
  ThreadItem,
  { type: "functionCallOutput" }
>;
type FunctionCallOutputContent = Exclude<
  FunctionCallOutputItem["output"],
  string
>[number];

function JsonBlock({ value }: { value: unknown }) {
  return <pre className="json-block">{JSON.stringify(value, null, 2)}</pre>;
}

function patchKindLabel(kind: FileChange["kind"]["type"]) {
  switch (kind) {
    case "add":
      return "ADD";
    case "delete":
      return "DELETE";
    case "update":
      return "EDIT";
  }
}

function fileChangeLabel(changes: FileChange[]) {
  const firstKind = changes[0]?.kind.type;
  if (!firstKind) {
    return "FILES";
  }

  if (changes.every((change) => change.kind.type === firstKind)) {
    return patchKindLabel(firstKind);
  }

  return "FILES";
}

function splitPath(path: string) {
  const slashIndex = path.lastIndexOf("/");
  if (slashIndex === -1) {
    return { directory: "", basename: path };
  }

  return {
    directory: path.slice(0, slashIndex),
    basename: path.slice(slashIndex + 1),
  };
}

function FilePathPreview({ path }: { path: string }) {
  const { directory, basename } = splitPath(path);

  return (
    <span className="collapsible-card-preview file-path-preview" title={path}>
      {directory && <span className="file-path-directory">{directory}/</span>}
      <span className="file-path-basename">{basename}</span>
    </span>
  );
}

function DisclosureSummary({ children }: { children: ReactNode }) {
  return (
    <summary>
      <span aria-hidden="true" className="collapsible-summary-icon" />
      {children}
    </summary>
  );
}

function functionCallName(item: FunctionCallOutputItem): string {
  return item.namespace ? `${item.namespace}.${item.name}` : item.name;
}

function FunctionCallOutputContentView({
  content,
  label,
}: {
  content: FunctionCallOutputContent;
  label: string;
}) {
  switch (content.type) {
    case "input_text":
      return <pre className="output-block">{content.text}</pre>;
    case "input_image":
      return (
        <img
          alt={`${label} output`}
          className="function-output-image"
          src={content.image_url}
        />
      );
    case "input_audio":
      return (
        <audio
          aria-label={`${label} output`}
          className="function-output-audio"
          controls
          src={content.audio_url}
        />
      );
    case "encrypted_content":
      return <p className="detail-meta">Encrypted output</p>;
  }
}

export function ThreadItemView({
  anchorId,
  item,
  onJumpToPreviousUserTurn,
  onUseSuggestedReply,
  sourceFileLinks,
  runtimeText,
}: Props) {
  switch (item.type) {
    case "userMessage":
      return (
        <article className="message-card user-card" id={anchorId}>
          {onJumpToPreviousUserTurn && (
            <div className="message-card-toolbar">
              <button
                aria-label="Jump to previous user turn"
                className="message-jump-button"
                title="Previous user turn"
                type="button"
                onClick={onJumpToPreviousUserTurn}
              >
                <svg
                  aria-hidden="true"
                  className="message-jump-icon"
                  fill="none"
                  focusable="false"
                  viewBox="0 0 24 24"
                >
                  <path d="M12 18V6" />
                  <path d="m6.5 11.5 5.5-5.5 5.5 5.5" />
                </svg>
              </button>
            </div>
          )}
          <div>
            {item.content.map((entry, index) => (
              <div key={`${item.id}-${index}`}>
                {entry.type === "text" ? (
                  <MarkdownBlock
                    sourceFileLinks={sourceFileLinks}
                    text={entry.text}
                  />
                ) : (
                  <p>{JSON.stringify(entry)}</p>
                )}
              </div>
            ))}
          </div>
        </article>
      );
    case "agentMessage":
      return (
        <article className="message-card agent-card">
          <header>Codex</header>
          {item.questions && item.questions.length > 0 ? (
            <div className="async-question-list">
              {item.questions.map((question, questionIndex) => (
                <section
                  className="async-question"
                  key={`${item.id}-question-${questionIndex}`}
                >
                  <p className="async-question-title">{question.title}</p>
                  {question.options && question.options.length > 0 && (
                    <div className="async-question-options">
                      {question.options.map((option, optionIndex) =>
                        onUseSuggestedReply ? (
                          <button
                            className="async-question-option"
                            key={`${item.id}-question-${questionIndex}-option-${optionIndex}`}
                            type="button"
                            onClick={() => onUseSuggestedReply(option)}
                          >
                            {option}
                          </button>
                        ) : (
                          <span
                            className="async-question-option-text"
                            key={`${item.id}-question-${questionIndex}-option-${optionIndex}`}
                          >
                            {option}
                          </span>
                        ),
                      )}
                    </div>
                  )}
                </section>
              ))}
            </div>
          ) : (
            <MarkdownBlock
              sourceFileLinks={sourceFileLinks}
              text={item.text || "Waiting for output..."}
            />
          )}
        </article>
      );
    case "functionCallOutput": {
      const label = functionCallName(item);
      return (
        <details className="detail-card collapsible-card">
          <DisclosureSummary>
            <span className="collapsible-summary-row">
              <span className="collapsible-card-label">Tool output</span>
              <span className="collapsible-card-preview mono" title={label}>
                {label}
              </span>
            </span>
          </DisclosureSummary>
          {typeof item.output === "string" ? (
            <pre className="output-block">{item.output}</pre>
          ) : (
            item.output.map((content, index) => (
              <FunctionCallOutputContentView
                content={content}
                key={`${item.id}-output-${index}`}
                label={label}
              />
            ))
          )}
        </details>
      );
    }
    case "noteToSelf":
      return (
        <article className="detail-card" id={anchorId}>
          <header>Note to self</header>
          <MarkdownBlock sourceFileLinks={sourceFileLinks} text={item.note} />
        </article>
      );
    case "reasoning":
      return (
        <article className="detail-card">
          <header>Reasoning</header>
          {item.summary.length > 0 && (
            <div>
              <h4>Summary</h4>
              {item.summary.map((entry, index) => (
                <p key={`${item.id}-summary-${index}`}>{entry}</p>
              ))}
            </div>
          )}
          {item.content.length > 0 && (
            <details className="detail-card collapsible-card">
              <DisclosureSummary>
                <span className="collapsible-summary-row">
                  <span className="collapsible-card-preview">Raw content</span>
                </span>
              </DisclosureSummary>
              {item.content.map((entry, index) => (
                <p key={`${item.id}-content-${index}`}>{entry}</p>
              ))}
            </details>
          )}
        </article>
      );
    case "commandExecution":
      return (
        <details className="detail-card collapsible-card">
          <DisclosureSummary>
            <span className="collapsible-summary-row">
              <span className="collapsible-card-label">EXEC</span>
              <span
                className="collapsible-card-preview mono"
                title={item.command}
              >
                {item.command}
              </span>
            </span>
          </DisclosureSummary>
          <p className="mono">{item.command}</p>
          <p className="detail-meta">cwd: {item.cwd}</p>
          <p className="detail-meta">status: {item.status}</p>
          {(item.aggregatedOutput || runtimeText) && (
            <pre className="output-block">
              {item.aggregatedOutput ?? runtimeText}
            </pre>
          )}
        </details>
      );
    case "fileChange":
      return (
        <details className="detail-card collapsible-card">
          <DisclosureSummary>
            <span className="collapsible-summary-row">
              <span className="collapsible-card-label">
                {fileChangeLabel(item.changes)}
              </span>
              {item.changes[0] ? (
                <FilePathPreview path={item.changes[0].path} />
              ) : (
                <span className="collapsible-card-preview">
                  No file changes
                </span>
              )}
              {item.changes.length > 1 && (
                <span className="collapsible-change-count">
                  +{item.changes.length - 1}
                </span>
              )}
            </span>
          </DisclosureSummary>
          <p className="detail-meta">status: {item.status}</p>
          {item.changes.map((change) => (
            <section key={`${item.id}-${change.path}`}>
              <h4>{change.path}</h4>
              <p className="detail-meta">kind: {change.kind.type}</p>
              <pre className="output-block">{change.diff}</pre>
            </section>
          ))}
          {runtimeText && <pre className="output-block">{runtimeText}</pre>}
        </details>
      );
    case "mcpToolCall":
      return (
        <article className="detail-card">
          <header>MCP tool call</header>
          <p>
            {item.server} / {item.tool}
          </p>
          <p className="detail-meta">status: {item.status}</p>
          {item.mcpAppResourceUri && (
            <p className="detail-meta">resource: {item.mcpAppResourceUri}</p>
          )}
          <JsonBlock value={item.arguments} />
          {item.result && <JsonBlock value={item.result} />}
          {item.error && <JsonBlock value={item.error} />}
        </article>
      );
    case "dynamicToolCall":
      return (
        <article className="detail-card">
          <header>Dynamic tool call</header>
          <p>{item.tool}</p>
          <p className="detail-meta">status: {item.status}</p>
          <JsonBlock value={item.arguments} />
          {item.contentItems && <JsonBlock value={item.contentItems} />}
        </article>
      );
    case "plan":
      return (
        <article className="detail-card">
          <header>Plan</header>
          <p>{item.text || "Waiting for plan output..."}</p>
        </article>
      );
    case "subAgentActivity":
      return (
        <article className="detail-card">
          <header>Subagent activity</header>
          <p className="detail-meta">status: {item.kind}</p>
          <p className="detail-meta">thread: {item.agentThreadId}</p>
          <p className="detail-meta">path: {item.agentPath}</p>
        </article>
      );
    case "sleep":
      return (
        <article className="detail-card">
          <header>Sleep</header>
          <p className="detail-meta">duration: {item.durationMs}ms</p>
        </article>
      );
    default:
      return (
        <article className="detail-card">
          <header>{item.type}</header>
          <JsonBlock value={item} />
        </article>
      );
  }
}
