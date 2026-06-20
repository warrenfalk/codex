export const SOURCE_FILE_READ_PATH = "/api/files/read";

export type SourceFileKind = "markdown" | "source" | "text";

export type SourceFileReadResponse = {
  content: string;
  displayPath: string;
  fileKind: SourceFileKind;
  language: string;
  line: number | null;
  path: string;
  root: string;
  sizeBytes: number;
};

export type SourceFileErrorResponse = {
  error: string;
  fileKind?: "image" | "unsupported";
  language?: string;
};
