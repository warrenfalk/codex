import { type ReactNode, useMemo, useState } from "react";

import type {
  AnyServerRequest,
  ChatgptAuthTokensRefreshResponse,
  CommandExecutionApprovalDecision,
  DynamicToolCallOutputContentItem,
  GrantedPermissionProfile,
  JsonValue,
  McpServerElicitationRequestResponse,
  PermissionGrantScope,
  ToolRequestUserInputResponse,
} from "@/types/protocol";

function isRequestMethod<TMethod extends AnyServerRequest["method"]>(
  request: AnyServerRequest,
  method: TMethod,
): request is Extract<AnyServerRequest, { method: TMethod }> {
  return request.method === method;
}

function JsonPreview({ value }: { value: unknown }) {
  return <pre className="json-block">{JSON.stringify(value, null, 2)}</pre>;
}

type CommandApprovalRequest = Extract<
  AnyServerRequest,
  { method: "item/commandExecution/requestApproval" }
>;

type CommandApprovalParamsCompat = CommandApprovalRequest["params"] & {
  additionalPermissions?: unknown;
  availableDecisions?: CommandExecutionApprovalDecision[] | null;
};

type FileSystemSpecialPathCompat =
  | { kind: "root" }
  | { kind: "minimal" }
  | { kind: "project_roots"; subpath: string | null }
  | { kind: "tmpdir" }
  | { kind: "slash_tmp" }
  | { kind: "unknown"; path: string; subpath: string | null };

type FileSystemPathCompat =
  | { type: "path"; path: string }
  | { type: "glob_pattern"; pattern: string }
  | { type: "special"; value: FileSystemSpecialPathCompat };

type FileSystemSandboxEntryCompat = {
  path: FileSystemPathCompat;
  access: "read" | "write" | "none";
};

type FileSystemPermissionsCompat = {
  read: string[] | null;
  write: string[] | null;
  globScanMaxDepth?: number;
  entries?: FileSystemSandboxEntryCompat[];
};

type PermissionsRequest = Extract<
  AnyServerRequest,
  { method: "item/permissions/requestApproval" }
>;

type PermissionsParamsCompat = Omit<
  PermissionsRequest["params"],
  "permissions"
> & {
  permissions: Omit<
    PermissionsRequest["params"]["permissions"],
    "fileSystem"
  > & {
    fileSystem: FileSystemPermissionsCompat | null;
  };
};

type PermissionsResponseCompat = {
  permissions: Omit<GrantedPermissionProfile, "fileSystem"> & {
    fileSystem?: FileSystemPermissionsCompat;
  };
  scope: PermissionGrantScope;
  strictAutoReview?: boolean;
};

function formatSpecialPath(path: FileSystemSpecialPathCompat): string {
  switch (path.kind) {
    case "root":
      return "root";
    case "minimal":
      return "minimal";
    case "project_roots":
      return path.subpath ? `project roots/${path.subpath}` : "project roots";
    case "tmpdir":
      return "temporary directory";
    case "slash_tmp":
      return "/tmp";
    case "unknown":
      return path.subpath ? `${path.path}/${path.subpath}` : path.path;
  }
}

function formatFileSystemPath(path: FileSystemPathCompat): string {
  switch (path.type) {
    case "path":
      return path.path;
    case "glob_pattern":
      return `glob ${path.pattern}`;
    case "special":
      return formatSpecialPath(path.value);
  }
}

function fileSystemEntryKey(entry: FileSystemSandboxEntryCompat): string {
  return JSON.stringify(entry);
}

function commandBasename(command: string): string {
  return command.split(/[\\/]/).pop() ?? command;
}

function formatShellArg(arg: string): string {
  if (/^[A-Za-z0-9_@%+=:,./-]+$/.test(arg)) {
    return arg;
  }
  return JSON.stringify(arg);
}

function escapeInlineCommand(command: string): string {
  return command.replace(/\r/g, "\\r").replace(/\n/g, "\\n");
}

function formatExecPolicyCommand(command: string[]): string {
  const [executable, flag, script] = command;
  if (
    executable &&
    flag === "-lc" &&
    script &&
    ["bash", "sh", "zsh"].includes(commandBasename(executable))
  ) {
    return escapeInlineCommand(script);
  }

  return command.map(formatShellArg).join(" ");
}

function commandDecisionLabel(
  decision: CommandExecutionApprovalDecision,
  params: CommandApprovalParamsCompat,
): string {
  if (typeof decision === "string") {
    switch (decision) {
      case "accept":
        return params.networkApprovalContext
          ? "Yes, just this once"
          : "Yes, proceed";
      case "acceptForSession":
        if (params.networkApprovalContext) {
          return "Yes, and allow this host for this conversation";
        }
        if (params.additionalPermissions) {
          return "Yes, and allow these permissions for this session";
        }
        return "Yes, and don't ask again for this command in this session";
      case "decline":
        return "No, continue without running it";
      case "cancel":
        return "No, and tell Codex what to do differently";
    }
  }

  if ("acceptWithExecpolicyAmendment" in decision) {
    const prefix = formatExecPolicyCommand(
      decision.acceptWithExecpolicyAmendment.execpolicy_amendment,
    );
    return `Yes, and don't ask again for commands that start with \`${prefix}\``;
  }

  const amendment =
    decision.applyNetworkPolicyAmendment.network_policy_amendment;
  if (amendment.action === "allow") {
    return `Yes, and allow \`${amendment.host}\` in the future`;
  }
  return `No, and block \`${amendment.host}\` in the future`;
}

function commandDecisionClassName(
  decision: CommandExecutionApprovalDecision,
): string {
  if (typeof decision === "string") {
    switch (decision) {
      case "accept":
      case "acceptForSession":
        return "approval-decision-button approval-decision-yes";
      case "decline":
      case "cancel":
        return "approval-decision-button approval-decision-no";
    }
  }

  if ("acceptWithExecpolicyAmendment" in decision) {
    return "approval-decision-button approval-decision-yes";
  }

  return decision.applyNetworkPolicyAmendment.network_policy_amendment
    .action === "allow"
    ? "approval-decision-button approval-decision-yes"
    : "approval-decision-button approval-decision-no";
}

function CardShell({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="request-card">
      <header>{title}</header>
      {children}
    </section>
  );
}

function CommandApprovalCard({
  request,
  onRespond,
}: RequestCardProps & {
  request: Extract<
    AnyServerRequest,
    { method: "item/commandExecution/requestApproval" }
  >;
}) {
  const params = request.params as CommandApprovalParamsCompat;
  const decisions = params.availableDecisions ?? [
    "accept",
    "acceptForSession",
    "decline",
    "cancel",
  ];

  return (
    <CardShell title="Command approval">
      {params.reason && <p>{params.reason}</p>}
      {params.command && <pre className="output-block">{params.command}</pre>}
      {params.cwd && <p className="detail-meta">cwd: {params.cwd}</p>}
      {params.commandActions && params.commandActions.length > 0 && (
        <JsonPreview value={params.commandActions} />
      )}
      {params.additionalPermissions && (
        <JsonPreview value={params.additionalPermissions} />
      )}
      <div className="button-row">
        {decisions.map((decision, index) => (
          <button
            key={`${request.id}-${index}`}
            className={commandDecisionClassName(decision)}
            type="button"
            onClick={() => onRespond(request, { decision })}
          >
            {commandDecisionLabel(decision, params)}
          </button>
        ))}
      </div>
    </CardShell>
  );
}

function FileChangeCard({
  request,
  onRespond,
}: RequestCardProps & {
  request: Extract<
    AnyServerRequest,
    { method: "item/fileChange/requestApproval" }
  >;
}) {
  return (
    <CardShell title="File change approval">
      {request.params.reason && <p>{request.params.reason}</p>}
      <div className="button-row">
        {(["accept", "acceptForSession", "decline", "cancel"] as const).map(
          (decision) => (
            <button
              key={decision}
              type="button"
              onClick={() => onRespond(request, { decision })}
            >
              {decision}
            </button>
          ),
        )}
      </div>
    </CardShell>
  );
}

function PermissionsCard({
  request,
  onRespond,
}: RequestCardProps & {
  request: Extract<
    AnyServerRequest,
    { method: "item/permissions/requestApproval" }
  >;
}) {
  const params = request.params as PermissionsParamsCompat;
  const { permissions } = params;
  const fileSystemEntries = permissions.fileSystem?.entries ?? [];
  const [networkEnabled, setNetworkEnabled] = useState(
    permissions.network?.enabled ?? false,
  );
  const [scope, setScope] = useState<PermissionGrantScope>("turn");
  const [selectedWrite, setSelectedWrite] = useState<string[]>(
    permissions.fileSystem?.write ?? [],
  );
  const [selectedRead, setSelectedRead] = useState<string[]>(
    permissions.fileSystem?.read ?? [],
  );
  const [selectedEntryKeys, setSelectedEntryKeys] = useState<string[]>(
    fileSystemEntries.map(fileSystemEntryKey),
  );

  const buildResponse = (denyAll: boolean) => {
    const response: PermissionsResponseCompat = {
      permissions: {},
      scope,
    };

    if (!denyAll && permissions.network && networkEnabled) {
      response.permissions.network = { enabled: true };
    }

    if (!denyAll && permissions.fileSystem) {
      const fileSystem: FileSystemPermissionsCompat = {
        read: null,
        write: null,
      };
      if (selectedRead.length > 0) {
        fileSystem.read = selectedRead;
      }
      if (selectedWrite.length > 0) {
        fileSystem.write = selectedWrite;
      }
      if (fileSystemEntries.length > 0) {
        fileSystem.entries = fileSystemEntries.filter((entry) =>
          selectedEntryKeys.includes(fileSystemEntryKey(entry)),
        );
      }
      if (typeof permissions.fileSystem.globScanMaxDepth === "number") {
        fileSystem.globScanMaxDepth = permissions.fileSystem.globScanMaxDepth;
      }
      response.permissions.fileSystem = fileSystem;
    }

    return response;
  };

  const togglePath = (
    values: string[],
    nextValue: string,
    setter: (next: string[]) => void,
  ) => {
    setter(
      values.includes(nextValue)
        ? values.filter((value) => value !== nextValue)
        : [...values, nextValue],
    );
  };

  const toggleEntry = (entry: FileSystemSandboxEntryCompat) => {
    togglePath(
      selectedEntryKeys,
      fileSystemEntryKey(entry),
      setSelectedEntryKeys,
    );
  };

  return (
    <CardShell title="Permission request">
      {request.params.reason && <p>{request.params.reason}</p>}
      {permissions.network && (
        <label className="field-row">
          <input
            checked={networkEnabled}
            type="checkbox"
            onChange={(event) => setNetworkEnabled(event.target.checked)}
          />
          Grant requested network access
        </label>
      )}
      {(permissions.fileSystem?.read ?? []).length > 0 && (
        <div>
          <h4>Readable paths</h4>
          {permissions.fileSystem?.read?.map((entry) => (
            <label key={entry} className="field-row">
              <input
                checked={selectedRead.includes(entry)}
                type="checkbox"
                onChange={() =>
                  togglePath(selectedRead, entry, setSelectedRead)
                }
              />
              <span className="mono">{entry}</span>
            </label>
          ))}
        </div>
      )}
      {(permissions.fileSystem?.write ?? []).length > 0 && (
        <div>
          <h4>Writable paths</h4>
          {permissions.fileSystem?.write?.map((entry) => (
            <label key={entry} className="field-row">
              <input
                checked={selectedWrite.includes(entry)}
                type="checkbox"
                onChange={() =>
                  togglePath(selectedWrite, entry, setSelectedWrite)
                }
              />
              <span className="mono">{entry}</span>
            </label>
          ))}
        </div>
      )}
      {fileSystemEntries.length > 0 && (
        <div>
          <h4>Filesystem entries</h4>
          {fileSystemEntries.map((entry) => {
            const key = fileSystemEntryKey(entry);
            return (
              <label key={key} className="field-row">
                <input
                  checked={selectedEntryKeys.includes(key)}
                  type="checkbox"
                  onChange={() => toggleEntry(entry)}
                />
                <span className="mono">
                  {entry.access}: {formatFileSystemPath(entry.path)}
                </span>
              </label>
            );
          })}
        </div>
      )}
      <label className="field-stack">
        <span>Grant scope</span>
        <select
          value={scope}
          onChange={(event) =>
            setScope(event.target.value as PermissionGrantScope)
          }
        >
          <option value="turn">Turn</option>
          <option value="session">Session</option>
        </select>
      </label>
      <div className="button-row">
        <button
          type="button"
          onClick={() => onRespond(request, buildResponse(false))}
        >
          Submit grant
        </button>
        <button
          type="button"
          onClick={() => onRespond(request, buildResponse(true))}
        >
          Deny all
        </button>
      </div>
    </CardShell>
  );
}

function RequestUserInputCard({
  request,
  onRespond,
}: RequestCardProps & {
  request: Extract<AnyServerRequest, { method: "item/tool/requestUserInput" }>;
}) {
  const [answers, setAnswers] = useState<Record<string, string>>({});

  const submit = () => {
    const normalizedAnswers = request.params.questions.map((question) => {
      const rawAnswer = answers[question.id] ?? "";
      return [
        question.id,
        {
          answers: rawAnswer
            ? rawAnswer
                .split("\n")
                .map((value) => value.trim())
                .filter(Boolean)
            : [],
        },
      ] as const;
    });

    const response: ToolRequestUserInputResponse = {
      answers: Object.fromEntries(normalizedAnswers),
    };
    onRespond(request, response);
  };

  return (
    <CardShell title="Tool input request">
      {request.params.questions.map((question) => (
        <div key={question.id} className="field-stack">
          <label htmlFor={question.id}>{question.header}</label>
          <p>{question.question}</p>
          {question.options?.map((option) => (
            <button
              key={`${question.id}-${option.label}`}
              type="button"
              className="option-chip"
              onClick={() =>
                setAnswers((current) => ({
                  ...current,
                  [question.id]: option.label,
                }))
              }
            >
              {option.label}
            </button>
          ))}
          <textarea
            id={question.id}
            rows={question.options ? 2 : 4}
            value={answers[question.id] ?? ""}
            onChange={(event) =>
              setAnswers((current) => ({
                ...current,
                [question.id]: event.target.value,
              }))
            }
          />
        </div>
      ))}
      <div className="button-row">
        <button type="button" onClick={submit}>
          Submit answers
        </button>
      </div>
    </CardShell>
  );
}

function defaultValueForSchemaProperty(
  schema: Record<string, unknown>,
): JsonValue {
  if (typeof schema.default !== "undefined") {
    return schema.default as JsonValue;
  }

  if (schema.type === "boolean") {
    return false;
  }

  if (schema.type === "number" || schema.type === "integer") {
    return 0;
  }

  if (schema.type === "array") {
    return [];
  }

  return "";
}

function McpFormFields({
  value,
  setValue,
  requestedSchema,
}: {
  value: Record<string, JsonValue>;
  setValue: (next: Record<string, JsonValue>) => void;
  requestedSchema: Extract<
    Extract<
      AnyServerRequest,
      { method: "mcpServer/elicitation/request" }
    >["params"],
    { mode: "form" }
  >["requestedSchema"];
}) {
  return (
    <div className="field-stack">
      {Object.entries(requestedSchema.properties).map(([name, schema]) => {
        const property = schema as Record<string, unknown>;
        const propertyType = property.type;

        if (propertyType === "boolean") {
          return (
            <label key={name} className="field-row">
              <input
                checked={Boolean(value[name])}
                type="checkbox"
                onChange={(event) =>
                  setValue({
                    ...value,
                    [name]: event.target.checked,
                  })
                }
              />
              {property.title?.toString() ?? name}
            </label>
          );
        }

        if (propertyType === "array") {
          const items = property.items as Record<string, unknown>;
          const rawOptions =
            (items.enum as string[] | undefined) ??
            (
              items.anyOf as
                | Array<{ const: string; title?: string }>
                | undefined
            )?.map((option) => option.const) ??
            [];
          const current = Array.isArray(value[name])
            ? (value[name] as string[])
            : [];

          return (
            <div key={name}>
              <span>{property.title?.toString() ?? name}</span>
              {rawOptions.map((option) => (
                <label key={`${name}-${option}`} className="field-row">
                  <input
                    checked={current.includes(option)}
                    type="checkbox"
                    onChange={() => {
                      const next = current.includes(option)
                        ? current.filter((entry) => entry !== option)
                        : [...current, option];
                      setValue({
                        ...value,
                        [name]: next,
                      });
                    }}
                  />
                  {option}
                </label>
              ))}
            </div>
          );
        }

        const rawSelectOptions =
          (property.enum as string[] | undefined) ??
          (
            property.oneOf as
              | Array<{ const: string; title?: string }>
              | undefined
          )?.map((option) => option.const) ??
          [];

        if (rawSelectOptions.length > 0) {
          return (
            <label key={name} className="field-stack">
              <span>{property.title?.toString() ?? name}</span>
              <select
                value={String(value[name] ?? "")}
                onChange={(event) =>
                  setValue({
                    ...value,
                    [name]: event.target.value,
                  })
                }
              >
                <option value="">Select one</option>
                {rawSelectOptions.map((option) => (
                  <option key={`${name}-${option}`} value={option}>
                    {option}
                  </option>
                ))}
              </select>
            </label>
          );
        }

        const inputType =
          property.format === "email"
            ? "email"
            : property.format === "uri"
              ? "url"
              : property.format === "date"
                ? "date"
                : "text";

        return (
          <label key={name} className="field-stack">
            <span>{property.title?.toString() ?? name}</span>
            <input
              type={
                propertyType === "number" || propertyType === "integer"
                  ? "number"
                  : inputType
              }
              value={String(value[name] ?? "")}
              onChange={(event) =>
                setValue({
                  ...value,
                  [name]:
                    propertyType === "number" || propertyType === "integer"
                      ? Number(event.target.value)
                      : event.target.value,
                })
              }
            />
          </label>
        );
      })}
    </div>
  );
}

function McpElicitationCard({
  request,
  onRespond,
}: RequestCardProps & {
  request: Extract<
    AnyServerRequest,
    { method: "mcpServer/elicitation/request" }
  >;
}) {
  const initialValue = useMemo(() => {
    if (request.params.mode !== "form") {
      return {};
    }

    return Object.fromEntries(
      Object.entries(request.params.requestedSchema.properties).map(
        ([name, schema]) => [
          name,
          defaultValueForSchemaProperty(schema as Record<string, unknown>),
        ],
      ),
    );
  }, [request.params]);
  const [content, setContent] =
    useState<Record<string, JsonValue>>(initialValue);
  const [metaText, setMetaText] = useState(
    request.params._meta ? JSON.stringify(request.params._meta, null, 2) : "",
  );

  const parsedMeta = useMemo(() => {
    if (!metaText.trim()) {
      return null;
    }

    try {
      return JSON.parse(metaText) as JsonValue;
    } catch {
      return null;
    }
  }, [metaText]);

  const respond = (action: "accept" | "decline" | "cancel") => {
    const response: McpServerElicitationRequestResponse = {
      action,
      content: action === "accept" ? content : null,
      _meta: parsedMeta,
    };
    onRespond(request, response);
  };

  return (
    <CardShell title="MCP elicitation">
      <p>{request.params.message}</p>
      {request.params.mode === "url" ? (
        <a href={request.params.url} rel="noreferrer" target="_blank">
          Open requested URL
        </a>
      ) : (
        <McpFormFields
          requestedSchema={request.params.requestedSchema}
          setValue={setContent}
          value={content}
        />
      )}
      <label className="field-stack">
        <span>Optional _meta JSON</span>
        <textarea
          rows={4}
          value={metaText}
          onChange={(event) => setMetaText(event.target.value)}
        />
      </label>
      <div className="button-row">
        <button type="button" onClick={() => respond("accept")}>
          Accept
        </button>
        <button type="button" onClick={() => respond("decline")}>
          Decline
        </button>
        <button type="button" onClick={() => respond("cancel")}>
          Cancel
        </button>
      </div>
    </CardShell>
  );
}

function DynamicToolCallCard({
  request,
  onRespond,
}: RequestCardProps & {
  request: Extract<AnyServerRequest, { method: "item/tool/call" }>;
}) {
  const [success, setSuccess] = useState(true);
  const [contentText, setContentText] = useState<string>(
    JSON.stringify(
      [
        {
          type: "inputText",
          text: "",
        } satisfies DynamicToolCallOutputContentItem,
      ],
      null,
      2,
    ),
  );

  const parsedContent = useMemo(() => {
    try {
      return JSON.parse(contentText) as DynamicToolCallOutputContentItem[];
    } catch {
      return null;
    }
  }, [contentText]);

  return (
    <CardShell title="Dynamic tool call">
      <p>{request.params.tool}</p>
      <JsonPreview value={request.params.arguments} />
      <label className="field-row">
        <input
          checked={success}
          type="checkbox"
          onChange={(event) => setSuccess(event.target.checked)}
        />
        Mark tool call successful
      </label>
      <label className="field-stack">
        <span>Content items JSON</span>
        <textarea
          rows={8}
          value={contentText}
          onChange={(event) => setContentText(event.target.value)}
        />
      </label>
      <div className="button-row">
        <button
          disabled={!parsedContent}
          type="button"
          onClick={() =>
            parsedContent &&
            onRespond(request, {
              success,
              contentItems: parsedContent,
            })
          }
        >
          Submit result
        </button>
      </div>
    </CardShell>
  );
}

function ChatgptRefreshCard({
  request,
  onRespond,
}: RequestCardProps & {
  request: Extract<
    AnyServerRequest,
    { method: "account/chatgptAuthTokens/refresh" }
  >;
}) {
  const [accessToken, setAccessToken] = useState("");
  const [accountId, setAccountId] = useState(
    request.params.previousAccountId ?? "",
  );
  const [planType, setPlanType] = useState("");

  const response: ChatgptAuthTokensRefreshResponse = {
    accessToken,
    chatgptAccountId: accountId,
    chatgptPlanType: planType || null,
  };

  return (
    <CardShell title="ChatGPT token refresh">
      <p>Reason: {request.params.reason}</p>
      {request.params.previousAccountId && (
        <p className="detail-meta">
          previous account: {request.params.previousAccountId}
        </p>
      )}
      <label className="field-stack">
        <span>Access token</span>
        <textarea
          rows={3}
          value={accessToken}
          onChange={(event) => setAccessToken(event.target.value)}
        />
      </label>
      <label className="field-stack">
        <span>Account id</span>
        <input
          value={accountId}
          onChange={(event) => setAccountId(event.target.value)}
        />
      </label>
      <label className="field-stack">
        <span>Plan type</span>
        <input
          value={planType}
          onChange={(event) => setPlanType(event.target.value)}
        />
      </label>
      <div className="button-row">
        <button
          disabled={!accessToken.trim() || !accountId.trim()}
          type="button"
          onClick={() => onRespond(request, response)}
        >
          Submit tokens
        </button>
      </div>
    </CardShell>
  );
}

function UnknownRequestCard({ request, onRespond }: RequestCardProps) {
  const [responseText, setResponseText] = useState("{}");
  const parsed = useMemo(() => {
    try {
      return JSON.parse(responseText) as unknown;
    } catch {
      return null;
    }
  }, [responseText]);

  return (
    <CardShell title={request.method}>
      <JsonPreview value={request.params} />
      <label className="field-stack">
        <span>Raw JSON response</span>
        <textarea
          rows={8}
          value={responseText}
          onChange={(event) => setResponseText(event.target.value)}
        />
      </label>
      <div className="button-row">
        <button
          disabled={parsed === null}
          type="button"
          onClick={() => parsed !== null && onRespond(request, parsed)}
        >
          Send response
        </button>
      </div>
    </CardShell>
  );
}

type RequestCardProps = {
  request: AnyServerRequest;
  onRespond: (request: AnyServerRequest, response: unknown) => void;
};

export function RequestCard({ request, onRespond }: RequestCardProps) {
  if (isRequestMethod(request, "item/commandExecution/requestApproval")) {
    return <CommandApprovalCard onRespond={onRespond} request={request} />;
  }

  if (isRequestMethod(request, "item/fileChange/requestApproval")) {
    return <FileChangeCard onRespond={onRespond} request={request} />;
  }

  if (isRequestMethod(request, "item/permissions/requestApproval")) {
    return <PermissionsCard onRespond={onRespond} request={request} />;
  }

  if (isRequestMethod(request, "item/tool/requestUserInput")) {
    return <RequestUserInputCard onRespond={onRespond} request={request} />;
  }

  if (isRequestMethod(request, "mcpServer/elicitation/request")) {
    return <McpElicitationCard onRespond={onRespond} request={request} />;
  }

  if (isRequestMethod(request, "item/tool/call")) {
    return <DynamicToolCallCard onRespond={onRespond} request={request} />;
  }

  if (isRequestMethod(request, "account/chatgptAuthTokens/refresh")) {
    return <ChatgptRefreshCard onRespond={onRespond} request={request} />;
  }

  return <UnknownRequestCard onRespond={onRespond} request={request} />;
}
