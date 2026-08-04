import type {
  PromptCategory,
  PromptContainer,
  PromptContainerInput,
  PromptDivider,
  PromptEntry,
  PromptItem,
} from "./promptTypes";
import {
  DEFAULT_GROUP_INTERVAL_MS,
  clampGroupIntervalMs,
  normalizePromptSendBehavior,
  normalizePromptTitle,
} from "./promptTypes";

export interface StorageAdapter {
  read(): Promise<string | null>;
  write(value: string): Promise<void>;
}

type PromptStoreDataV1 = {
  version: 1;
  prompts: PromptItem[];
};

type LegacyPromptContainer = Omit<PromptContainer, "categoryId" | "sendBehavior"> & {
  categoryId?: string;
  sendBehavior?: unknown;
};

type PromptStoreDataV2 = {
  version: 2;
  containers: LegacyPromptContainer[];
};

type PromptStoreDataV3 = {
  version: 3;
  categories: PromptCategory[];
  containers: LegacyPromptContainer[];
  activeCategoryId: string | null;
};

type PromptStoreDataV4 = {
  version: 4;
  categories: PromptCategory[];
  containers: LegacyPromptContainer[];
  dividers: Partial<PromptDivider>[];
  activeCategoryId: string | null;
};

type PromptStoreData = PromptStoreDataV1 | PromptStoreDataV2 | PromptStoreDataV3 | PromptStoreDataV4;

export type PromptLibrarySnapshot = {
  categories: PromptCategory[];
  containers: PromptContainer[];
  dividers: PromptDivider[];
  activeCategoryId: string;
};

export type CombineSinglesResult = {
  container: PromptContainer;
  snapshot: PromptLibrarySnapshot;
};

export type SplitGroupResult = {
  containers: PromptContainer[];
  snapshot: PromptLibrarySnapshot;
};

type NormalizedPromptStore = PromptLibrarySnapshot;

type RawPromptStore = {
  categories?: Partial<PromptCategory>[];
  containers?: LegacyPromptContainer[];
  dividers?: Partial<PromptDivider>[];
  activeCategoryId?: string | null;
};

export const DEFAULT_CATEGORY_ID = "category-default";
const DEFAULT_CATEGORY_NAME = "Default";

function generateId(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
}

function nowIso(): string {
  return new Date().toISOString();
}

function defaultCategory(now: string): PromptCategory {
  return {
    id: DEFAULT_CATEGORY_ID,
    name: DEFAULT_CATEGORY_NAME,
    order: 0,
    createdAt: now,
    updatedAt: now,
  };
}

function sortCategories(categories: PromptCategory[]): PromptCategory[] {
  return [...categories].sort((a, b) => a.order - b.order);
}

function sortContainers(containers: PromptContainer[]): PromptContainer[] {
  return [...containers].sort((a, b) => a.order - b.order);
}

function sortDividers(dividers: PromptDivider[]): PromptDivider[] {
  return [...dividers].sort((a, b) => a.order - b.order);
}

function sortEntries(entries: PromptEntry[]): PromptEntry[] {
  return [...entries].sort((a, b) => a.order - b.order);
}

function entryFromBody(
  body: string,
  order: number,
  id = generateId("entry"),
  title?: string,
  sendBehavior?: PromptContainerInput["sendBehavior"]
): PromptEntry {
  return {
    id,
    ...(title?.trim() ? { title: title.trim() } : {}),
    ...(sendBehavior ? { sendBehavior: normalizePromptSendBehavior(sendBehavior) } : {}),
    body: body.trim(),
    order,
  };
}

function normalizeEntries(
  prompts: PromptContainerInput["prompts"]
): PromptEntry[] {
  return prompts
    .map((prompt, index) => entryFromBody(
      prompt.body,
      prompt.order ?? index,
      prompt.id,
      prompt.title,
      prompt.sendBehavior
    ))
    .filter((prompt) => prompt.body.length > 0)
    .sort((a, b) => a.order - b.order)
    .map((prompt, index) => ({ ...prompt, order: index }));
}

function normalizeContainer(
  input: PromptContainerInput,
  order: number,
  now: string,
  categoryId: string,
  existing?: PromptContainer | LegacyPromptContainer
): PromptContainer {
  const title = normalizePromptTitle(input.title);
  const prompts = normalizeEntries(input.prompts);
  if (!title || prompts.length === 0) {
    throw new Error("Invalid prompt container data");
  }

  const type = input.type === "group" ? "group" : "single";
  const usablePrompts = type === "single" ? [prompts[0]] : prompts;

  return {
    id: existing?.id ?? generateId("container"),
    categoryId,
    title,
    type,
    sendBehavior: normalizePromptSendBehavior(input.sendBehavior ?? existing?.sendBehavior),
    prompts: usablePrompts.map((prompt, index) => ({ ...prompt, order: index })),
    intervalMs: clampGroupIntervalMs(input.intervalMs ?? existing?.intervalMs),
    order,
    createdAt: existing?.createdAt ?? now,
    updatedAt: now,
  };
}

function normalizeCategory(
  category: Partial<PromptCategory>,
  order: number,
  now: string
): PromptCategory {
  const name = String(category.name ?? "").trim();
  if (!name) throw new Error("Invalid category name");
  return {
    id: category.id || generateId("category"),
    name,
    order,
    createdAt: category.createdAt || now,
    updatedAt: category.updatedAt || now,
  };
}

function normalizeDivider(
  divider: Partial<PromptDivider>,
  order: number,
  now: string,
  categoryId: string
): PromptDivider {
  const label = typeof divider.label === "string" ? divider.label : "";
  return {
    id: divider.id || generateId("divider"),
    categoryId,
    label,
    order,
    createdAt: divider.createdAt || now,
    updatedAt: divider.updatedAt || now,
  };
}

function containerToInput(container: PromptContainer | LegacyPromptContainer): PromptContainerInput {
  return {
    title: container.title,
    type: container.type,
    sendBehavior: normalizePromptSendBehavior(container.sendBehavior),
    prompts: sortEntries(container.prompts).map((prompt) => ({
      id: prompt.id,
      title: prompt.title,
      sendBehavior: prompt.sendBehavior,
      body: prompt.body,
      order: prompt.order,
    })),
    intervalMs: container.intervalMs,
    categoryId: container.categoryId,
  };
}

function legacyPromptToContainer(
  prompt: PromptItem,
  categoryId: string
): PromptContainer {
  return {
    id: prompt.id,
    categoryId,
    title: prompt.title,
    type: "single",
    sendBehavior: "inherit",
    prompts: [entryFromBody(prompt.body, 0, `${prompt.id}-entry`)],
    intervalMs: DEFAULT_GROUP_INTERVAL_MS,
    order: prompt.order,
    createdAt: prompt.createdAt,
    updatedAt: prompt.updatedAt,
  };
}

function normalizeStore(raw: RawPromptStore): NormalizedPromptStore {
  const now = nowIso();
  const fallback = defaultCategory(now);
  const categories = sortCategories(
    (raw.categories && raw.categories.length > 0 ? raw.categories : [fallback])
      .map((category, index) => {
        const order = typeof category.order === "number" && Number.isFinite(category.order)
          ? category.order
          : index;
        return normalizeCategory(category, order, now);
      })
  ).map((category, index) => ({ ...category, order: index }));

  const categoryIds = new Set(categories.map((category) => category.id));
  const fallbackCategoryId = categories[0]?.id ?? DEFAULT_CATEGORY_ID;
  const containers = sortContainers(
    (raw.containers ?? []).map((container, index) => {
      const rawCategoryId = typeof container.categoryId === "string"
        ? container.categoryId
        : undefined;
      const categoryId = rawCategoryId && categoryIds.has(rawCategoryId)
        ? rawCategoryId
        : fallbackCategoryId;
      const order = typeof container.order === "number" && Number.isFinite(container.order)
        ? container.order
        : index;
      const updatedAt = container.updatedAt || now;
      return normalizeContainer(
        containerToInput(container),
        order,
        updatedAt,
        categoryId,
        {
          ...container,
          categoryId,
          createdAt: container.createdAt || now,
          updatedAt,
        }
      );
    })
  );

  const dividers = sortDividers(
    (raw.dividers ?? []).map((divider, index) => {
      const rawCategoryId = typeof divider.categoryId === "string"
        ? divider.categoryId
        : undefined;
      const categoryId = rawCategoryId && categoryIds.has(rawCategoryId)
        ? rawCategoryId
        : fallbackCategoryId;
      const order = typeof divider.order === "number" && Number.isFinite(divider.order)
        ? divider.order
        : index;
      return normalizeDivider(divider, order, now, categoryId);
    })
  );

  const activeCategoryId =
    raw.activeCategoryId && categoryIds.has(raw.activeCategoryId)
      ? raw.activeCategoryId
      : fallbackCategoryId;

  return {
    categories,
    containers,
    dividers,
    activeCategoryId,
  };
}

function parseStore(data: string | null): NormalizedPromptStore {
  const now = nowIso();
  const fallback = defaultCategory(now);
  if (!data) {
    return {
      categories: [fallback],
      containers: [],
      dividers: [],
      activeCategoryId: fallback.id,
    };
  }

  try {
    const parsed = JSON.parse(data) as PromptStoreData;
    if (parsed.version === 1 && Array.isArray(parsed.prompts)) {
      return normalizeStore({
        categories: [fallback],
        containers: parsed.prompts.map((prompt) =>
          legacyPromptToContainer(prompt, fallback.id)
        ),
        activeCategoryId: fallback.id,
      });
    }
    if (parsed.version === 2 && Array.isArray(parsed.containers)) {
      return normalizeStore({
        categories: [fallback],
        containers: parsed.containers.map((container) => ({
          ...container,
          categoryId: fallback.id,
        })),
        activeCategoryId: fallback.id,
      });
    }
    if (
      parsed.version === 3 &&
      Array.isArray(parsed.categories) &&
      Array.isArray(parsed.containers)
    ) {
      return normalizeStore({
        categories: parsed.categories,
        containers: parsed.containers,
        activeCategoryId: parsed.activeCategoryId ?? undefined,
      });
    }
    if (
      parsed.version === 4 &&
      Array.isArray(parsed.categories) &&
      Array.isArray(parsed.containers)
    ) {
      return normalizeStore({
        categories: parsed.categories,
        containers: parsed.containers,
        dividers: Array.isArray(parsed.dividers) ? parsed.dividers : [],
        activeCategoryId: parsed.activeCategoryId ?? undefined,
      });
    }
  } catch {
    return {
      categories: [fallback],
      containers: [],
      dividers: [],
      activeCategoryId: fallback.id,
    };
  }

  return {
    categories: [fallback],
    containers: [],
    dividers: [],
    activeCategoryId: fallback.id,
  };
}

function validateImportedData(json: string): NormalizedPromptStore {
  const parsed = JSON.parse(json) as PromptStoreData;
  const now = nowIso();
  const fallback = defaultCategory(now);

  if (parsed.version === 1 && Array.isArray(parsed.prompts)) {
    const store = normalizeStore({
      categories: [fallback],
      containers: parsed.prompts.map((prompt) =>
        legacyPromptToContainer(prompt, fallback.id)
      ),
      activeCategoryId: fallback.id,
    });
    store.containers.forEach((container) => {
      normalizeContainer(
        containerToInput(container),
        container.order,
        container.updatedAt,
        container.categoryId,
        container
      );
    });
    return store;
  }

  if (parsed.version === 2 && Array.isArray(parsed.containers)) {
    const store = normalizeStore({
      categories: [fallback],
      containers: parsed.containers.map((container) => ({
        ...container,
        categoryId: fallback.id,
      })),
      activeCategoryId: fallback.id,
    });
    store.containers.forEach((container) => {
      normalizeContainer(
        containerToInput(container),
        container.order,
        container.updatedAt,
        container.categoryId,
        container
      );
    });
    return store;
  }

  if (
    parsed.version === 3 &&
    Array.isArray(parsed.categories) &&
    Array.isArray(parsed.containers)
  ) {
    return normalizeStore({
      categories: parsed.categories,
      containers: parsed.containers,
      activeCategoryId: parsed.activeCategoryId ?? undefined,
    });
  }

  if (
    parsed.version === 4 &&
    Array.isArray(parsed.categories) &&
    Array.isArray(parsed.containers)
  ) {
    return normalizeStore({
      categories: parsed.categories,
      containers: parsed.containers,
      dividers: Array.isArray(parsed.dividers) ? parsed.dividers : [],
      activeCategoryId: parsed.activeCategoryId ?? undefined,
    });
  }

  throw new Error("Invalid format");
}

export function validatePromptLibraryJson(json: string): void {
  validateImportedData(json);
}

function serializeStore(store: NormalizedPromptStore): string {
  const data: PromptStoreDataV4 = {
    version: 4,
    categories: sortCategories(store.categories),
    containers: sortContainers(store.containers),
    dividers: sortDividers(store.dividers),
    activeCategoryId: store.activeCategoryId,
  };
  return JSON.stringify(data, null, 2);
}

export function createPromptStore(adapter: StorageAdapter) {
  async function load(): Promise<NormalizedPromptStore> {
    return parseStore(await adapter.read());
  }

  async function save(store: NormalizedPromptStore): Promise<PromptLibrarySnapshot> {
    const snapshot = normalizeStore(store);
    await adapter.write(serializeStore(snapshot));
    return snapshot;
  }

  async function resolveCategoryId(
    store: NormalizedPromptStore,
    categoryId?: string
  ): Promise<string> {
    if (categoryId && store.categories.some((category) => category.id === categoryId)) {
      return categoryId;
    }
    if (store.categories.some((category) => category.id === store.activeCategoryId)) {
      return store.activeCategoryId;
    }
    return store.categories[0]?.id ?? DEFAULT_CATEGORY_ID;
  }

  return {
    async getData(): Promise<NormalizedPromptStore> {
      return load();
    },

    async list(): Promise<PromptContainer[]> {
      return sortContainers((await load()).containers);
    },

    async listCategories(): Promise<PromptCategory[]> {
      return sortCategories((await load()).categories);
    },

    async getActiveCategoryId(): Promise<string> {
      return (await load()).activeCategoryId;
    },

    async setActiveCategoryId(categoryId: string): Promise<void> {
      const store = await load();
      if (!store.categories.some((category) => category.id === categoryId)) {
        throw new Error("Unknown category");
      }
      await save({ ...store, activeCategoryId: categoryId });
    },

    async createCategory(name: string): Promise<PromptCategory> {
      const trimmedName = name.trim();
      if (!trimmedName) throw new Error("Invalid category name");
      const store = await load();
      const now = nowIso();
      const maxOrder = store.categories.reduce(
        (max, category) => Math.max(max, category.order),
        -1
      );
      const category: PromptCategory = {
        id: generateId("category"),
        name: trimmedName,
        order: maxOrder + 1,
        createdAt: now,
        updatedAt: now,
      };
      await save({
        ...store,
        categories: [...store.categories, category],
        activeCategoryId: category.id,
      });
      return category;
    },

    async renameCategory(id: string, name: string): Promise<PromptCategory | null> {
      const trimmedName = name.trim();
      if (!trimmedName) throw new Error("Invalid category name");
      const store = await load();
      let updatedCategory: PromptCategory | null = null;
      const now = nowIso();
      const categories = store.categories.map((category) => {
        if (category.id !== id) return category;
        updatedCategory = {
          ...category,
          name: trimmedName,
          updatedAt: now,
        };
        return updatedCategory;
      });
      if (!updatedCategory) return null;
      await save({ ...store, categories });
      return updatedCategory;
    },

    async removeCategory(id: string): Promise<void> {
      const store = await load();
      if (store.categories.length <= 1) {
        throw new Error("Cannot remove last category");
      }
      if (store.containers.some((container) => container.categoryId === id)) {
        throw new Error("Cannot remove category with prompts");
      }
      const categories = store.categories
        .filter((category) => category.id !== id)
        .map((category, index) => ({ ...category, order: index }));
      if (categories.length === store.categories.length) return;
      await save({
        ...store,
        categories,
        dividers: store.dividers.filter((divider) => divider.categoryId !== id),
        activeCategoryId:
          store.activeCategoryId === id
            ? categories[0]?.id ?? DEFAULT_CATEGORY_ID
            : store.activeCategoryId,
      });
    },

    async create(input: {
      title: string;
      body: string;
      sendBehavior?: PromptContainerInput["sendBehavior"];
      categoryId?: string;
    }): Promise<PromptContainer> {
      const store = await load();
      const categoryId = await resolveCategoryId(store, input.categoryId);
      const maxOrder = store.containers
        .filter((container) => container.categoryId === categoryId)
        .reduce((max, p) => Math.max(max, p.order), -1);
      const now = nowIso();
      const container = normalizeContainer(
        {
          title: input.title,
          type: "single",
          prompts: [{ body: input.body }],
          intervalMs: DEFAULT_GROUP_INTERVAL_MS,
          sendBehavior: input.sendBehavior,
          categoryId,
        },
        maxOrder + 1,
        now,
        categoryId
      );
      await save({
        ...store,
        containers: [...store.containers, container],
      });
      return container;
    },

    async createGroup(input: {
      title: string;
      prompts: PromptContainerInput["prompts"];
      intervalMs?: number;
      sendBehavior?: PromptContainerInput["sendBehavior"];
      categoryId?: string;
    }): Promise<PromptContainer> {
      const store = await load();
      const categoryId = await resolveCategoryId(store, input.categoryId);
      const maxOrder = store.containers
        .filter((container) => container.categoryId === categoryId)
        .reduce((max, p) => Math.max(max, p.order), -1);
      const now = nowIso();
      const container = normalizeContainer(
        {
          title: input.title,
          type: "group",
          prompts: input.prompts,
          intervalMs: input.intervalMs,
          sendBehavior: input.sendBehavior,
          categoryId,
        },
        maxOrder + 1,
        now,
        categoryId
      );
      await save({
        ...store,
        containers: [...store.containers, container],
      });
      return container;
    },

    async update(
      id: string,
      input: {
        title?: string;
        body?: string;
        type?: "single" | "group";
        prompts?: PromptContainerInput["prompts"];
        intervalMs?: number;
        sendBehavior?: PromptContainerInput["sendBehavior"];
      }
    ): Promise<PromptContainer | null> {
      const store = await load();
      const idx = store.containers.findIndex((p) => p.id === id);
      if (idx === -1) return null;
      const existing = store.containers[idx];
      const now = nowIso();
      const updated = normalizeContainer(
        {
          title: input.title ?? existing.title,
          type: input.type ?? existing.type,
          prompts: input.prompts ?? (
            input.body === undefined
              ? existing.prompts
              : [{ id: existing.prompts[0]?.id, body: input.body, order: 0 }]
          ),
          intervalMs: input.intervalMs ?? existing.intervalMs,
          sendBehavior: input.sendBehavior ?? existing.sendBehavior,
          categoryId: existing.categoryId,
        },
        existing.order,
        now,
        existing.categoryId,
        existing
      );
      const containers = [...store.containers];
      containers[idx] = updated;
      await save({ ...store, containers });
      return updated;
    },

    async remove(id: string): Promise<void> {
      const store = await load();
      await save({
        ...store,
        containers: store.containers.filter((p) => p.id !== id),
      });
    },

    async createDivider(input: {
      categoryId?: string;
      label?: string;
    }): Promise<PromptDivider> {
      const store = await load();
      const categoryId = await resolveCategoryId(store, input.categoryId);
      const maxOrder = [
        ...store.containers.filter((c) => c.categoryId === categoryId),
        ...store.dividers.filter((d) => d.categoryId === categoryId),
      ].reduce((max, item) => Math.max(max, item.order), -1);
      const now = nowIso();
      const divider = normalizeDivider(
        { label: input.label ?? "" },
        maxOrder + 1,
        now,
        categoryId
      );
      await save({
        ...store,
        dividers: [...store.dividers, divider],
      });
      return divider;
    },

    async updateDivider(
      id: string,
      input: { label?: string }
    ): Promise<PromptDivider | null> {
      const store = await load();
      const idx = store.dividers.findIndex((d) => d.id === id);
      if (idx === -1) return null;
      const existing = store.dividers[idx];
      const now = nowIso();
      const updated: PromptDivider = {
        ...existing,
        label: input.label === undefined ? existing.label : input.label,
        updatedAt: now,
      };
      const dividers = [...store.dividers];
      dividers[idx] = updated;
      await save({ ...store, dividers });
      return updated;
    },

    async removeDivider(id: string): Promise<void> {
      const store = await load();
      await save({
        ...store,
        dividers: store.dividers.filter((d) => d.id !== id),
      });
    },

    async combineSingles(input: {
      ids: string[];
      title: string;
      deleteOriginals: boolean;
    }): Promise<CombineSinglesResult> {
      const uniqueIds = [...new Set(input.ids)];
      if (uniqueIds.length < 2) {
        throw new Error("Select at least two prompts");
      }

      const store = await load();
      const byId = new Map(store.containers.map((container) => [container.id, container]));
      const selected = uniqueIds.map((id) => byId.get(id));
      if (selected.some((container) => !container || container.type !== "single")) {
        throw new Error("Only single prompts can be combined");
      }

      const singles = selected as PromptContainer[];
      const categoryId = singles[0].categoryId;
      if (singles.some((container) => container.categoryId !== categoryId)) {
        throw new Error("Prompts must belong to the same category");
      }

      const now = nowIso();
      const sendBehavior = singles.every(
        (container) => container.sendBehavior === singles[0].sendBehavior
      ) ? singles[0].sendBehavior : "inherit";
      const group = normalizeContainer(
        {
          title: input.title,
          type: "group",
          sendBehavior,
          prompts: singles.map((container) => ({
            title: container.title,
            sendBehavior: container.sendBehavior,
            body: sortEntries(container.prompts)[0].body,
          })),
          intervalMs: DEFAULT_GROUP_INTERVAL_MS,
          categoryId,
        },
        0,
        now,
        categoryId
      );

      const categoryContainers = sortContainers(
        store.containers.filter((container) => container.categoryId === categoryId)
      );
      const selectedIdSet = new Set(uniqueIds);
      const earliestSelectedIndex = categoryContainers.findIndex((container) =>
        selectedIdSet.has(container.id)
      );
      const nextCategoryContainers = input.deleteOriginals
        ? categoryContainers.filter((container) => !selectedIdSet.has(container.id))
        : [...categoryContainers];
      const insertIndex = input.deleteOriginals
        ? Math.max(0, earliestSelectedIndex)
        : nextCategoryContainers.length;
      nextCategoryContainers.splice(insertIndex, 0, group);

      const normalizedCategory = nextCategoryContainers.map((container, order) => ({
        ...container,
        order,
        updatedAt: container.id === group.id ? container.updatedAt : now,
      }));
      const snapshot = await save({
        ...store,
        containers: [
          ...store.containers.filter((container) => container.categoryId !== categoryId),
          ...normalizedCategory,
        ],
      });
      return {
        container: { ...group, order: insertIndex },
        snapshot,
      };
    },

    async splitGroup(id: string): Promise<SplitGroupResult> {
      const store = await load();
      const group = store.containers.find((container) => container.id === id);
      if (!group || group.type !== "group") {
        throw new Error("Prompt group not found");
      }

      const now = nowIso();
      const entries = sortEntries(group.prompts);
      const singles = entries.map((entry, index) => normalizeContainer(
        {
          title: entry.title?.trim() || `${group.title} ${index + 1}`,
          type: "single",
          sendBehavior: entry.sendBehavior ?? group.sendBehavior,
          prompts: [{ body: entry.body }],
          intervalMs: DEFAULT_GROUP_INTERVAL_MS,
          categoryId: group.categoryId,
        },
        0,
        now,
        group.categoryId
      ));

      const categoryContainers = sortContainers(
        store.containers.filter((container) => container.categoryId === group.categoryId)
      );
      const groupIndex = categoryContainers.findIndex((container) => container.id === id);
      categoryContainers.splice(groupIndex, 1, ...singles);
      const normalizedCategory = categoryContainers.map((container, order) => ({
        ...container,
        order,
        updatedAt: singles.some((single) => single.id === container.id)
          ? container.updatedAt
          : now,
      }));
      const snapshot = await save({
        ...store,
        containers: [
          ...store.containers.filter((container) => container.categoryId !== group.categoryId),
          ...normalizedCategory,
        ],
      });
      return {
        containers: normalizedCategory.filter((container) =>
          singles.some((single) => single.id === container.id)
        ),
        snapshot,
      };
    },

    async reorder(orderedIds: string[], categoryId?: string): Promise<void> {
      const store = await load();
      const targetCategoryId = categoryId ?? store.activeCategoryId;
      const targetContainers = store.containers.filter(
        (container) => container.categoryId === targetCategoryId
      );
      const targetDividers = store.dividers.filter(
        (divider) => divider.categoryId === targetCategoryId
      );
      const untouchedContainers = store.containers.filter(
        (container) => container.categoryId !== targetCategoryId
      );
      const untouchedDividers = store.dividers.filter(
        (divider) => divider.categoryId !== targetCategoryId
      );
      const containerMap = new Map(targetContainers.map((p) => [p.id, p]));
      const dividerMap = new Map(targetDividers.map((d) => [d.id, d]));

      type MixedItem =
        | { kind: "container"; data: PromptContainer }
        | { kind: "divider"; data: PromptDivider };
      const ordered: MixedItem[] = [];
      const seenContainerIds = new Set<string>();
      const seenDividerIds = new Set<string>();
      for (const id of orderedIds) {
        if (dividerMap.has(id)) {
          seenDividerIds.add(id);
          ordered.push({ kind: "divider", data: dividerMap.get(id)! });
        } else if (containerMap.has(id)) {
          seenContainerIds.add(id);
          ordered.push({ kind: "container", data: containerMap.get(id)! });
        }
      }
      for (const container of targetContainers) {
        if (!seenContainerIds.has(container.id)) {
          ordered.push({ kind: "container", data: container });
        }
      }
      for (const divider of targetDividers) {
        if (!seenDividerIds.has(divider.id)) {
          ordered.push({ kind: "divider", data: divider });
        }
      }

      const now = nowIso();
      const normalizedContainers: PromptContainer[] = [];
      const normalizedDividers: PromptDivider[] = [];
      ordered.forEach((item, i) => {
        if (item.kind === "container") {
          normalizedContainers.push({ ...item.data, order: i, updatedAt: now });
        } else {
          normalizedDividers.push({ ...item.data, order: i, updatedAt: now });
        }
      });
      await save({
        ...store,
        containers: [...untouchedContainers, ...normalizedContainers],
        dividers: [...untouchedDividers, ...normalizedDividers],
      });
    },

    async exportJson(): Promise<string> {
      return serializeStore(await load());
    },

    async importJson(json: string): Promise<void> {
      await save(validateImportedData(json));
    },
  };
}
