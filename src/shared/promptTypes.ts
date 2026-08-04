export type PromptItem = {
  id: string;
  title: string;
  body: string;
  order: number;
  createdAt: string;
  updatedAt: string;
};

export type PromptContainerType = "single" | "group";
export type PromptSendBehavior = "inherit" | "paste_only" | "paste_enter" | "paste_command_enter";

export type PromptCategory = {
  id: string;
  name: string;
  order: number;
  createdAt: string;
  updatedAt: string;
};

export type PromptEntry = {
  id: string;
  title?: string;
  sendBehavior?: PromptSendBehavior;
  body: string;
  order: number;
};

export type PromptContainer = {
  id: string;
  categoryId: string;
  title: string;
  type: PromptContainerType;
  sendBehavior: PromptSendBehavior;
  prompts: PromptEntry[];
  intervalMs: number;
  order: number;
  createdAt: string;
  updatedAt: string;
};

export type PromptContainerInput = {
  title: string;
  type: PromptContainerType;
  sendBehavior?: PromptSendBehavior;
  prompts: Array<{
    id?: string;
    title?: string;
    sendBehavior?: PromptSendBehavior;
    body: string;
    order?: number;
  }>;
  intervalMs?: number;
  categoryId?: string;
};

export type PromptDivider = {
  id: string;
  categoryId: string;
  label: string;
  order: number;
  createdAt: string;
  updatedAt: string;
};

export const DEFAULT_GROUP_INTERVAL_MS = 700;
export const MIN_GROUP_INTERVAL_MS = 200;
export const MAX_GROUP_INTERVAL_MS = 3000;

export function normalizePromptTitle(title: string): string {
  return title.trim();
}

export function clampGroupIntervalMs(value: number | undefined): number {
  if (!Number.isFinite(value)) return DEFAULT_GROUP_INTERVAL_MS;
  return Math.min(
    MAX_GROUP_INTERVAL_MS,
    Math.max(MIN_GROUP_INTERVAL_MS, Math.round(value ?? DEFAULT_GROUP_INTERVAL_MS))
  );
}

export function normalizePromptSendBehavior(value: unknown): PromptSendBehavior {
  if (
    value === "paste_only" ||
    value === "paste_enter" ||
    value === "paste_command_enter"
  ) {
    return value;
  }
  return "inherit";
}

export function getPromptPreview(body: string, maxLength = 80): string {
  const compact = body.replace(/\s+/g, " ").trim();
  if (compact.length <= maxLength) return compact;
  return `${compact.slice(0, maxLength).trimEnd()}...`;
}

export function getPromptContainerBodies(container: PromptContainer): string[] {
  return [...container.prompts]
    .sort((a, b) => a.order - b.order)
    .map((prompt) => prompt.body.trim())
    .filter(Boolean);
}

export function getPromptContainerPreviewLines(container: PromptContainer): string[] {
  const bodies = getPromptContainerBodies(container).map((body) =>
    body.replace(/\s+/g, " ").trim()
  );

  if (container.type === "group") {
    return bodies.slice(0, 2).map((body, index) => `${index + 1}. ${body}`);
  }

  return bodies.slice(0, 1);
}

export function getPromptContainerPreview(
  container: PromptContainer,
  maxLength = 120
): string {
  const bodies = getPromptContainerBodies(container);
  if (container.type === "group") {
    return getPromptPreview(
      bodies.map((body, index) => `${index + 1}. ${body}`).join(" "),
      maxLength
    );
  }
  return getPromptPreview(bodies[0] ?? "", maxLength);
}

export function getPromptContainerMeta(container: PromptContainer): string | null {
  const count = getPromptContainerBodies(container).length;
  if (container.type === "group") {
    return `Group · ${count} prompts`;
  }
  return null;
}
