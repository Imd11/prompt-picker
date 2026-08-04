import {
  useEffect,
  useRef,
  useState,
  type DragEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import { Reorder, useDragControls } from "motion/react";
import type {
  PromptCategory,
  PromptContainer,
  PromptSendBehavior,
} from "../shared/promptTypes";
import {
  DEFAULT_GROUP_INTERVAL_MS,
  MAX_GROUP_INTERVAL_MS,
  MIN_GROUP_INTERVAL_MS,
  getPromptContainerBodies,
  getPromptContainerPreviewLines,
} from "../shared/promptTypes";
import type { Messages } from "../shared/i18n";
import { CategoryRail } from "./CategoryRail";

type EditorMode = "single" | "group";

type DraftPrompt = {
  id?: string;
  title?: string;
  sendBehavior?: PromptSendBehavior;
  body: string;
};

type Draft = {
  title: string;
  type: EditorMode;
  body: string;
  prompts: DraftPrompt[];
  intervalMs: number;
};

interface PromptManagerProps {
  prompts: PromptContainer[];
  categories: PromptCategory[];
  activeCategoryId: string | null;
  categoryCounts: Record<string, number>;
  totalPromptCount: number;
  messages: Messages;
  onOpenSettings: () => void;
  onSelectCategory: (categoryId: string) => void;
  onCreateCategory: (name: string) => void | Promise<void>;
  onRenameCategory: (categoryId: string, name: string) => void | Promise<void>;
  onDeleteCategory: (categoryId: string) => void | Promise<void>;
  getCategoryDisplayName: (category: PromptCategory) => string;
  categoryActionError?: string | null;
  onDraftActivityChange?: (active: boolean) => void;
  onCreate: (input: {
    title: string;
    body: string;
  }) => void | Promise<void>;
  onCreateGroup: (input: {
    title: string;
    prompts: Array<{
      id?: string;
      title?: string;
      sendBehavior?: PromptSendBehavior;
      body: string;
      order: number;
    }>;
    intervalMs: number;
  }) => void | Promise<void>;
  onUpdate: (
    id: string,
    input: {
      title?: string;
      body?: string;
      type?: EditorMode;
      sendBehavior?: PromptSendBehavior;
      prompts?: Array<{
        id?: string;
        title?: string;
        sendBehavior?: PromptSendBehavior;
        body: string;
        order: number;
      }>;
      intervalMs?: number;
    }
  ) => void | Promise<void>;
  onCombineSingles: (input: {
    ids: string[];
    title: string;
    deleteOriginals: boolean;
  }) => void | Promise<void>;
  onSplitGroup: (id: string) => void | Promise<void>;
  onDelete: (id: string) => void | Promise<void>;
  onReorder: (orderedIds: string[]) => void | Promise<void>;
  onImport: () => void | Promise<void>;
  onExport: () => void | Promise<void>;
}

const emptyDraft = (): Draft => ({
  title: "",
  type: "single",
  body: "",
  prompts: [{ body: "" }, { body: "" }],
  intervalMs: DEFAULT_GROUP_INTERVAL_MS,
});

function draftFromPrompt(prompt: PromptContainer): Draft {
  const orderedPrompts = [...prompt.prompts].sort((a, b) => a.order - b.order);
  return {
    title: prompt.title,
    type: prompt.type,
    body: orderedPrompts[0]?.body ?? "",
    prompts: prompt.type === "group"
      ? orderedPrompts.map((entry) => ({
          id: entry.id,
          title: entry.title,
          sendBehavior: entry.sendBehavior,
          body: entry.body,
        }))
      : [{ body: orderedPrompts[0]?.body ?? "" }, { body: "" }],
    intervalMs: prompt.intervalMs,
  };
}

function cleanPrompts(
  prompts: DraftPrompt[]
): Array<{
  id?: string;
  title?: string;
  sendBehavior?: PromptSendBehavior;
  body: string;
  order: number;
}> {
  return prompts
    .map((prompt) => ({ ...prompt, body: prompt.body.trim() }))
    .filter((prompt) => Boolean(prompt.body))
    .map((prompt, order) => ({ ...prompt, order }));
}

function hasValidDraft(draft: Draft): boolean {
  if (!draft.title.trim()) return false;
  if (draft.type === "single") return Boolean(draft.body.trim());
  return cleanPrompts(draft.prompts).length > 0;
}

const GROUP_INTERVAL_STEP_SECONDS = 0.1;

function formatIntervalSeconds(intervalMs: number): string {
  return String(Number((intervalMs / 1000).toFixed(2)));
}

function intervalSecondsToMs(seconds: number): number {
  if (!Number.isFinite(seconds)) return MIN_GROUP_INTERVAL_MS;
  const minSeconds = MIN_GROUP_INTERVAL_MS / 1000;
  const maxSeconds = MAX_GROUP_INTERVAL_MS / 1000;
  const clampedSeconds = Math.min(maxSeconds, Math.max(minSeconds, seconds));
  return Math.round(clampedSeconds * 1000);
}

function moveArrayItem<T>(items: T[], from: number, to: number): T[] {
  if (from === to || from < 0 || to < 0 || from >= items.length || to >= items.length) {
    return items;
  }
  const next = [...items];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

function sameOrder(left: string[], right: string[]): boolean {
  return left.length === right.length && left.every((id, index) => id === right[index]);
}

function reconcilePromptIds(current: string[], prompts: PromptContainer[]): string[] {
  const availableIds = new Set(prompts.map((prompt) => prompt.id));
  const next = current.filter((id) => availableIds.has(id));
  const includedIds = new Set(next);
  for (const prompt of prompts) {
    if (!includedIds.has(prompt.id)) next.push(prompt.id);
  }
  return next;
}

type PromptReorderRowProps = {
  id: string;
  enabled: boolean;
  className: string;
  onDragStart: () => void;
  onDragEnd: () => void;
  children: ReactNode;
};

function isInteractivePromptTarget(target: EventTarget | null): boolean {
  return target instanceof Element && Boolean(target.closest(
    "button, a, input, textarea, select, [contenteditable='true'], [role='menuitem']"
  ));
}

function PromptReorderRow({
  id,
  enabled,
  className,
  onDragStart,
  onDragEnd,
  children,
}: PromptReorderRowProps) {
  const dragControls = useDragControls();

  const handlePointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!enabled || isInteractivePromptTarget(event.target)) return;
    dragControls.start(event);
  };

  return (
    <Reorder.Item
      as="div"
      value={id}
      role="listitem"
      data-reorder-id={id}
      layout="position"
      dragListener={false}
      dragControls={dragControls}
      dragMomentum={false}
      dragElastic={0.04}
      onPointerDown={handlePointerDown}
      onDragStart={onDragStart}
      onDragEnd={onDragEnd}
      whileDrag={{
        scale: 1.01,
        boxShadow: "0 12px 26px rgba(15, 23, 42, 0.16)",
      }}
      transition={{
        layout: { type: "spring", stiffness: 520, damping: 42 },
      }}
      className={className}
    >
      {children}
    </Reorder.Item>
  );
}

function PromptKindBadge({ prompt, messages }: { prompt: PromptContainer; messages: Messages }) {
  if (prompt.type !== "group") return null;
  const count = getPromptContainerBodies(prompt).length;
  return (
    <span className="prompt-kind-badge">
      {messages.manager.groupMeta(count)}
    </span>
  );
}

function SendBehaviorBadge({ prompt, messages }: { prompt: PromptContainer; messages: Messages }) {
  if (prompt.sendBehavior !== "paste_only" && prompt.sendBehavior !== "paste_enter") {
    return null;
  }
  const label = prompt.sendBehavior === "paste_only"
    ? messages.settings.pasteOnly
    : messages.settings.pasteEnter;
  return <span className="prompt-send-badge">{label}</span>;
}

export function PromptManager({
  prompts,
  categories,
  activeCategoryId,
  categoryCounts,
  totalPromptCount,
  messages,
  onOpenSettings,
  onSelectCategory,
  onCreateCategory,
  onRenameCategory,
  onDeleteCategory,
  getCategoryDisplayName,
  categoryActionError = null,
  onDraftActivityChange,
  onCreate,
  onCreateGroup,
  onUpdate,
  onCombineSingles,
  onSplitGroup,
  onDelete,
  onReorder,
  onImport,
  onExport
}: PromptManagerProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [deleteConfirmId, setDeleteConfirmId] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>(() => emptyDraft());
  const [editDraft, setEditDraft] = useState<Draft>(() => emptyDraft());
  const [createPanelOpen, setCreatePanelOpen] = useState(false);
  const [createToastMessage, setCreateToastMessage] = useState<string | null>(null);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<string[]>([]);
  const [mergeIds, setMergeIds] = useState<string[]>([]);
  const [mergeTitle, setMergeTitle] = useState("");
  const [deleteOriginals, setDeleteOriginals] = useState(true);
  const [openMenuId, setOpenMenuId] = useState<string | null>(null);
  const [splitConfirmId, setSplitConfirmId] = useState<string | null>(null);
  const [draggingMergeId, setDraggingMergeId] = useState<string | null>(null);
  const [draggingPromptId, setDraggingPromptId] = useState<string | null>(null);
  const [orderedPromptIds, setOrderedPromptIds] = useState<string[]>(() =>
    prompts.map((prompt) => prompt.id)
  );
  const [reorderPending, setReorderPending] = useState(false);
  const [reorderError, setReorderError] = useState<string | null>(null);
  const [groupActionPending, setGroupActionPending] = useState<"combine" | "split" | null>(null);
  const [groupActionError, setGroupActionError] = useState<string | null>(null);
  const titleInputRef = useRef<HTMLInputElement | null>(null);
  const bodyTextareaRef = useRef<HTMLTextAreaElement | null>(null);
  const groupPromptRefs = useRef<Array<HTMLTextAreaElement | null>>([]);
  const editTitleInputRef = useRef<HTMLInputElement | null>(null);
  const editBodyTextareaRef = useRef<HTMLTextAreaElement | null>(null);
  const editGroupPromptRefs = useRef<Array<HTMLTextAreaElement | null>>([]);
  const submitGuardRef = useRef(false);
  const submitGuardTimerRef = useRef<number | null>(null);
  const createToastTimerRef = useRef<number | null>(null);
  const mergeTitleInputRef = useRef<HTMLInputElement | null>(null);
  const mergeDialogOpenRef = useRef(false);
  const groupActionPendingRef = useRef(false);
  const orderedPromptIdsRef = useRef(orderedPromptIds);
  const reorderPendingRef = useRef(false);

  useEffect(() => {
    return () => {
      if (submitGuardTimerRef.current !== null) {
        window.clearTimeout(submitGuardTimerRef.current);
      }
      if (createToastTimerRef.current !== null) {
        window.clearTimeout(createToastTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    setEditingId(null);
    setDeleteConfirmId(null);
    setCreatePanelOpen(false);
    setDraft(emptyDraft());
    setSelectionMode(false);
    setSelectedIds([]);
    setMergeIds([]);
    setOpenMenuId(null);
    setSplitConfirmId(null);
    setGroupActionError(null);
    setReorderError(null);
  }, [activeCategoryId]);

  useEffect(() => {
    if (draggingPromptId !== null || reorderPending) return;
    const next = prompts.map((prompt) => prompt.id);
    orderedPromptIdsRef.current = next;
    setOrderedPromptIds((current) => sameOrder(current, next) ? current : next);
  }, [draggingPromptId, prompts, reorderPending]);

  useEffect(() => {
    if (!openMenuId) return;
    const closeMenu = () => setOpenMenuId(null);
    const closeMenuOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeMenu();
    };
    window.addEventListener("pointerdown", closeMenu);
    window.addEventListener("keydown", closeMenuOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeMenu);
      window.removeEventListener("keydown", closeMenuOnEscape);
    };
  }, [openMenuId]);

  useEffect(() => {
    const isOpen = mergeIds.length > 0;
    if (isOpen && !mergeDialogOpenRef.current) {
      window.requestAnimationFrame(() => {
        mergeTitleInputRef.current?.focus();
        mergeTitleInputRef.current?.select();
      });
    }
    mergeDialogOpenRef.current = isOpen;
  }, [mergeIds.length]);

  const hasDraftActivity = createPanelOpen || editingId !== null || deleteConfirmId !== null
    || selectionMode || mergeIds.length > 0 || splitConfirmId !== null;

  useEffect(() => {
    onDraftActivityChange?.(hasDraftActivity);
  }, [hasDraftActivity, onDraftActivityChange]);

  const setDraftPrompt = (index: number, value: string) => {
    const next = [...draft.prompts];
    next[index] = { ...next[index], body: value };
    setDraft({ ...draft, prompts: next });
  };

  const setEditPrompt = (index: number, value: string) => {
    const next = [...editDraft.prompts];
    next[index] = { ...next[index], body: value };
    setEditDraft({ ...editDraft, prompts: next });
  };

  const draftFromCreateDom = (): Draft => ({
    ...draft,
    title: titleInputRef.current?.value ?? draft.title,
    body: bodyTextareaRef.current?.value ?? draft.body,
    prompts: draft.prompts.map((prompt, index) => ({
      ...prompt,
      body: groupPromptRefs.current[index]?.value ?? prompt.body,
    })),
  });

  const draftFromEditDom = (): Draft => ({
    ...editDraft,
    title: editTitleInputRef.current?.value ?? editDraft.title,
    body: editBodyTextareaRef.current?.value ?? editDraft.body,
    prompts: editDraft.prompts.map((prompt, index) => ({
      ...prompt,
      body: editGroupPromptRefs.current[index]?.value ?? prompt.body,
    })),
  });

  const runSubmitOnce = (callback: () => void | Promise<void>) => {
    if (submitGuardRef.current) return;
    submitGuardRef.current = true;
    if (submitGuardTimerRef.current !== null) {
      window.clearTimeout(submitGuardTimerRef.current);
    }
    submitGuardTimerRef.current = window.setTimeout(() => {
      submitGuardRef.current = false;
      submitGuardTimerRef.current = null;
    }, 250);
    Promise.resolve(callback()).catch((error) => {
      console.error("Prompt manager action failed:", error);
    });
  };

  const showCreateToast = (message: string) => {
    setCreateToastMessage(message);
    if (createToastTimerRef.current !== null) {
      window.clearTimeout(createToastTimerRef.current);
    }
    createToastTimerRef.current = window.setTimeout(() => {
      setCreateToastMessage(null);
      createToastTimerRef.current = null;
    }, 2200);
  };

  const handleCreate = async (sourceDraft = draft) => {
    if (!hasValidDraft(sourceDraft)) return;
    if (sourceDraft.type === "group") {
      await onCreateGroup({
        title: sourceDraft.title.trim(),
        prompts: cleanPrompts(sourceDraft.prompts),
        intervalMs: sourceDraft.intervalMs,
      });
      showCreateToast(messages.manager.groupAdded);
    } else {
      await onCreate({
        title: sourceDraft.title.trim(),
        body: sourceDraft.body.trim(),
      });
      showCreateToast(messages.manager.promptAdded);
    }
    setDraft(emptyDraft());
    setCreatePanelOpen(false);
  };

  const openCreatePanel = () => {
    setCreatePanelOpen(true);
    window.requestAnimationFrame(() => {
      titleInputRef.current?.focus();
      titleInputRef.current?.select();
    });
  };

  const cancelCreate = () => {
    setDraft(emptyDraft());
    setCreatePanelOpen(false);
  };

  const handleSaveEdit = async (id: string, sourceDraft = editDraft) => {
    if (!hasValidDraft(sourceDraft)) return;
    if (sourceDraft.type === "group") {
      await onUpdate(id, {
        title: sourceDraft.title.trim(),
        type: "group",
        prompts: cleanPrompts(sourceDraft.prompts),
        intervalMs: sourceDraft.intervalMs,
      });
    } else {
      await onUpdate(id, {
        title: sourceDraft.title.trim(),
        type: "single",
        body: sourceDraft.body.trim(),
      });
    }
    setEditingId(null);
    setEditDraft(emptyDraft());
  };

  const visiblePromptIds = reconcilePromptIds(orderedPromptIds, prompts);
  const promptById = new Map(prompts.map((prompt) => [prompt.id, prompt]));
  const visiblePrompts = visiblePromptIds
    .map((id) => promptById.get(id))
    .filter((prompt): prompt is PromptContainer => Boolean(prompt));

  const updatePromptOrder = (next: string[]) => {
    orderedPromptIdsRef.current = next;
    setOrderedPromptIds(next);
  };

  const persistPromptOrder = async (next: string[], fallback: string[]) => {
    if (reorderPendingRef.current || sameOrder(next, fallback)) return;
    reorderPendingRef.current = true;
    setReorderPending(true);
    setReorderError(null);
    try {
      await onReorder(next);
    } catch (error) {
      console.error("Failed to reorder prompts:", error);
      orderedPromptIdsRef.current = fallback;
      setOrderedPromptIds(fallback);
      setReorderError(messages.manager.reorderFailed);
    } finally {
      reorderPendingRef.current = false;
      setReorderPending(false);
    }
  };

  const handleMoveUp = (index: number) => {
    if (index === 0 || reorderPendingRef.current) return;
    const fallback = prompts.map((prompt) => prompt.id);
    const next = moveArrayItem(visiblePromptIds, index, index - 1);
    updatePromptOrder(next);
    void persistPromptOrder(next, fallback);
  };

  const handleMoveDown = (index: number) => {
    if (index === visiblePromptIds.length - 1 || reorderPendingRef.current) return;
    const fallback = prompts.map((prompt) => prompt.id);
    const next = moveArrayItem(visiblePromptIds, index, index + 1);
    updatePromptOrder(next);
    void persistPromptOrder(next, fallback);
  };

  const startSelection = () => {
    if (groupActionPendingRef.current) return;
    setEditingId(null);
    setDeleteConfirmId(null);
    setCreatePanelOpen(false);
    setOpenMenuId(null);
    setSelectionMode(true);
    setSelectedIds([]);
    setGroupActionError(null);
  };

  const cancelSelection = () => {
    setSelectionMode(false);
    setSelectedIds([]);
    setMergeIds([]);
    setGroupActionError(null);
  };

  const toggleSelection = (id: string) => {
    setSelectedIds((current) => current.includes(id)
      ? current.filter((selectedId) => selectedId !== id)
      : [...current, id]);
  };

  const openMergeDialog = () => {
    if (groupActionPendingRef.current) return;
    const selected = prompts.filter((prompt) =>
      prompt.type === "single" && selectedIds.includes(prompt.id)
    );
    if (selected.length < 2) return;
    setMergeIds(selected.map((prompt) => prompt.id));
    setMergeTitle(messages.manager.newGroupDefaultName);
    setDeleteOriginals(true);
    setGroupActionError(null);
  };

  const handleCombine = async () => {
    if (mergeIds.length < 2 || !mergeTitle.trim() || groupActionPendingRef.current) return;
    groupActionPendingRef.current = true;
    setGroupActionPending("combine");
    setGroupActionError(null);
    try {
      await onCombineSingles({
        ids: mergeIds,
        title: mergeTitle.trim(),
        deleteOriginals,
      });
      cancelSelection();
      showCreateToast(messages.manager.groupCombined);
    } catch (error) {
      console.error("Failed to combine prompts:", error);
      setGroupActionError(messages.manager.combineFailed);
    } finally {
      groupActionPendingRef.current = false;
      setGroupActionPending(null);
    }
  };

  const handleSplit = async (id: string) => {
    if (groupActionPendingRef.current) return;
    groupActionPendingRef.current = true;
    setGroupActionPending("split");
    setGroupActionError(null);
    try {
      await onSplitGroup(id);
      setSplitConfirmId(null);
      showCreateToast(messages.manager.groupSplit);
    } catch (error) {
      console.error("Failed to split prompt group:", error);
      setGroupActionError(messages.manager.splitFailed);
    } finally {
      groupActionPendingRef.current = false;
      setGroupActionPending(null);
    }
  };

  const moveMergeItem = (sourceId: string, targetId: string) => {
    if (groupActionPendingRef.current) return;
    const from = mergeIds.indexOf(sourceId);
    const to = mergeIds.indexOf(targetId);
    if (from === -1 || to === -1) return;
    setMergeIds(moveArrayItem(mergeIds, from, to));
  };

  const moveMergeItemBy = (index: number, offset: number) => {
    if (groupActionPendingRef.current) return;
    const targetIndex = index + offset;
    if (targetIndex < 0 || targetIndex >= mergeIds.length) return;
    setMergeIds((ids) => moveArrayItem(ids, index, targetIndex));
  };

  const closeMergeDialog = () => {
    if (groupActionPendingRef.current) return;
    setMergeIds([]);
    setGroupActionError(null);
  };

  const closeSplitDialog = () => {
    if (groupActionPendingRef.current) return;
    setSplitConfirmId(null);
    setGroupActionError(null);
  };

  const mergePrompts = mergeIds
    .map((id) => prompts.find((prompt) => prompt.id === id))
    .filter((prompt): prompt is PromptContainer => Boolean(prompt));
  const splitPrompt = prompts.find((prompt) => prompt.id === splitConfirmId) ?? null;
  const groupActionBusy = groupActionPending !== null;
  const groupDialogOpen = mergeIds.length > 0 || splitPrompt !== null;
  const promptReorderEnabled = visiblePrompts.length > 1
    && editingId === null
    && deleteConfirmId === null
    && !selectionMode
    && !groupDialogOpen
    && !groupActionBusy
    && !reorderPending;

  const handlePromptDragEnd = () => {
    const next = orderedPromptIdsRef.current;
    const fallback = prompts.map((prompt) => prompt.id);
    setDraggingPromptId(null);
    void persistPromptOrder(next, fallback);
  };

  return (
    <div className="prompt-manager page-stack">
      <header className="page-header" inert={groupDialogOpen}>
        <div>
          <h1>{messages.manager.title}</h1>
          <p>{messages.manager.count(totalPromptCount)}</p>
        </div>
        <div className="toolbar">
          <button className="button button-secondary" onClick={onOpenSettings}>
            {messages.common.settings}
          </button>
          <button className="button button-secondary" onClick={onImport}>
            {messages.common.import}
          </button>
          <button className="button button-secondary" onClick={onExport}>
            {messages.common.export}
          </button>
        </div>
      </header>

      <div className="prompt-manager-body">
        <div className="manager-dialog-background" inert={groupDialogOpen}>
          <CategoryRail
            categories={categories}
            activeCategoryId={activeCategoryId}
            counts={categoryCounts}
            messages={{
              title: messages.manager.categoriesTitle,
              newCategory: messages.manager.newCategory,
              newCategoryName: messages.manager.newCategoryName,
              newCategoryDefaultName: messages.manager.newCategoryDefaultName,
              categoryActions: messages.manager.categoryActions,
              renameCategory: messages.manager.renameCategory,
              deleteCategory: messages.manager.deleteCategory,
              saveCategory: messages.manager.saveCategory,
              cancelCategory: messages.manager.cancelCategory,
            }}
            getCategoryDisplayName={getCategoryDisplayName}
            actionError={categoryActionError}
            onSelect={onSelectCategory}
            onCreate={onCreateCategory}
            onRename={onRenameCategory}
            onDelete={onDeleteCategory}
          />
        </div>
        <div className="prompt-manager-content">
          <section className="list-panel">
            <div className="manager-dialog-background" inert={groupDialogOpen}>
            <div className="section-heading panel-heading-with-actions">
              <div>
                <h2>{messages.manager.promptListTitle}</h2>
              </div>
              <div className="list-heading-actions">
                {!selectionMode ? (
                  <button
                    className="button button-secondary"
                    type="button"
                    onClick={startSelection}
                    disabled={prompts.filter((prompt) => prompt.type === "single").length < 2}
                  >
                    {messages.manager.selectPrompts}
                  </button>
                ) : null}
                <button
                  className="button button-primary list-add-button"
                  type="button"
                  onClick={openCreatePanel}
                  disabled={selectionMode}
                >
                  + {messages.manager.addPrompt}
                </button>
              </div>
            </div>
            {createPanelOpen ? (
              <form
                className="editor-panel editor-panel-stacked create-panel-inline"
                onSubmit={(event) => {
                  event.preventDefault();
                  runSubmitOnce(() => handleCreate(draftFromCreateDom()));
                }}
              >
                <div className="section-heading panel-heading-with-actions">
                  <div>
                    <h2>{messages.manager.newContainerTitle}</h2>
                  </div>
                  <div
                    className="segmented-control"
                    aria-label={messages.manager.promptContainerType}
                  >
                    <button
                      className={draft.type === "single" ? "is-selected" : ""}
                      type="button"
                      aria-pressed={draft.type === "single"}
                      onClick={() => setDraft({ ...draft, type: "single" })}
                    >
                      {messages.manager.single}
                    </button>
                    <button
                      className={draft.type === "group" ? "is-selected" : ""}
                      type="button"
                      aria-pressed={draft.type === "group"}
                      onClick={() => setDraft({ ...draft, type: "group" })}
                    >
                      {messages.manager.group}
                    </button>
                  </div>
                </div>
                <input
                  ref={titleInputRef}
                  className="field"
                  type="text"
                  placeholder={messages.manager.titlePlaceholder}
                  value={draft.title}
                  onChange={(e) => setDraft({ ...draft, title: e.target.value })}
                />
                {draft.type === "single" ? (
                  <textarea
                    ref={bodyTextareaRef}
                    className="field prompt-body-field"
                    placeholder={messages.manager.bodyPlaceholder}
                    value={draft.body}
                    onChange={(e) => setDraft({ ...draft, body: e.target.value })}
                  />
                ) : (
                  <GroupFields
                    prompts={draft.prompts}
                    intervalMs={draft.intervalMs}
                    messages={messages}
                    promptRef={(index, node) => {
                      groupPromptRefs.current[index] = node;
                    }}
                    onIntervalChange={(intervalMs) => setDraft({ ...draft, intervalMs })}
                    onPromptChange={setDraftPrompt}
                    onInsertPrompt={(index) => {
                      const next = [...draft.prompts];
                      next.splice(index + 1, 0, { body: "" });
                      setDraft({ ...draft, prompts: next });
                    }}
                    onRemovePrompt={(index) => {
                      const next = draft.prompts.filter((_, i) => i !== index);
                      setDraft({ ...draft, prompts: next.length ? next : [{ body: "" }] });
                    }}
                    onMovePrompt={(from, to) => {
                      setDraft({ ...draft, prompts: moveArrayItem(draft.prompts, from, to) });
                    }}
                  />
                )}
                <div className="editor-submit-row">
                  <button
                    className="button button-secondary"
                    type="button"
                    onClick={cancelCreate}
                  >
                    {messages.manager.cancel}
                  </button>
                  <button
                    className="button button-primary editor-submit"
                    type="submit"
                    onPointerDown={(event) => event.preventDefault()}
                    onPointerUp={() => runSubmitOnce(() => handleCreate(draftFromCreateDom()))}
                  >
                    {draft.type === "group" ? messages.manager.addGroup : messages.manager.addPrompt}
                  </button>
                </div>
              </form>
            ) : null}
            {createToastMessage ? (
              <div className="create-toast" role="status" aria-live="polite">
                <span className="create-toast-icon" aria-hidden="true">✓</span>
                <span>{createToastMessage}</span>
              </div>
            ) : null}
            {selectionMode ? (
              <div className="selection-toolbar">
                <strong role="status" aria-live="polite">
                  {messages.manager.selectedCount(selectedIds.length)}
                </strong>
                <div className="selection-toolbar-actions">
                  <button
                    className="button button-primary"
                    type="button"
                    disabled={groupActionBusy || selectedIds.length < 2}
                    onClick={openMergeDialog}
                  >
                    {messages.manager.combineIntoGroup}
                  </button>
                  <button
                    className="button button-secondary"
                    type="button"
                    disabled={groupActionBusy}
                    onClick={cancelSelection}
                  >
                    {messages.manager.cancel}
                  </button>
                </div>
              </div>
            ) : null}
            {reorderError ? (
              <div className="prompt-reorder-error" role="alert">
                {reorderError}
              </div>
            ) : null}
            <Reorder.Group
              as="div"
              axis="y"
              values={visiblePromptIds}
              onReorder={updatePromptOrder}
              className="prompt-list"
              layoutScroll
              role="list"
              data-reorder-list="true"
            >
              {prompts.length === 0 ? (
                <div className="empty-state-block">
                  {activeCategoryId ? messages.manager.emptyCategory : messages.manager.noPrompts}
                </div>
              ) : visiblePrompts.map((prompt, index) => (
                <PromptReorderRow
                  key={prompt.id}
                  id={prompt.id}
                  enabled={promptReorderEnabled}
                  onDragStart={() => {
                    setOpenMenuId(null);
                    setDraggingPromptId(prompt.id);
                  }}
                  onDragEnd={handlePromptDragEnd}
                  className={`prompt-item ${prompt.type === "group" ? "prompt-item-group" : ""} ${selectedIds.includes(prompt.id) ? "is-selected" : ""} ${promptReorderEnabled ? "is-reorder-enabled" : ""} ${draggingPromptId === prompt.id ? "is-dragging" : ""}`}
                >
                  {selectionMode ? (
                    <label
                      className={`prompt-selection-row ${prompt.type === "group" ? "is-unavailable" : ""}`}
                    >
                      <span className="prompt-selection-control">
                        <input
                          type="checkbox"
                          checked={selectedIds.includes(prompt.id)}
                          disabled={groupActionBusy || prompt.type === "group"}
                          aria-label={messages.manager.selectPrompt(prompt.title)}
                          onChange={() => toggleSelection(prompt.id)}
                        />
                      </span>
                      <span className="prompt-info">
                        <span className="prompt-title-row">
                          <strong>{prompt.title}</strong>
                          <PromptKindBadge prompt={prompt} messages={messages} />
                        </span>
                        <span className="prompt-preview-lines">
                          {getPromptContainerPreviewLines(prompt).map((line) => (
                            <span className="prompt-preview-line" key={line}>{line}</span>
                          ))}
                        </span>
                      </span>
                    </label>
                  ) : editingId === prompt.id ? (
                    <form
                      className="edit-form edit-form-stacked"
                      onSubmit={(event) => {
                        event.preventDefault();
                        runSubmitOnce(() => handleSaveEdit(prompt.id, draftFromEditDom()));
                      }}
                    >
                      <div
                        className="segmented-control"
                        aria-label={messages.manager.editPromptContainerType}
                      >
                        <button
                          className={editDraft.type === "single" ? "is-selected" : ""}
                          type="button"
                          aria-pressed={editDraft.type === "single"}
                          onClick={() => setEditDraft({ ...editDraft, type: "single" })}
                        >
                          {messages.manager.single}
                        </button>
                        <button
                          className={editDraft.type === "group" ? "is-selected" : ""}
                          type="button"
                          aria-pressed={editDraft.type === "group"}
                          onClick={() => setEditDraft({ ...editDraft, type: "group" })}
                        >
                          {messages.manager.group}
                        </button>
                      </div>
                      <input
                        ref={editTitleInputRef}
                        className="field"
                        type="text"
                        value={editDraft.title}
                        onChange={(e) => setEditDraft({ ...editDraft, title: e.target.value })}
                      />
                      {editDraft.type === "single" ? (
                        <textarea
                          ref={editBodyTextareaRef}
                          className="field prompt-body-field"
                          value={editDraft.body}
                          onChange={(e) => setEditDraft({ ...editDraft, body: e.target.value })}
                        />
                      ) : (
                        <GroupFields
                          prompts={editDraft.prompts}
                          intervalMs={editDraft.intervalMs}
                          messages={messages}
                          promptRef={(entryIndex, node) => {
                            editGroupPromptRefs.current[entryIndex] = node;
                          }}
                          onIntervalChange={(intervalMs) =>
                            setEditDraft({ ...editDraft, intervalMs })
                          }
                          onPromptChange={setEditPrompt}
                          onInsertPrompt={(entryIndex) => {
                            const next = [...editDraft.prompts];
                            next.splice(entryIndex + 1, 0, { body: "" });
                            setEditDraft({ ...editDraft, prompts: next });
                          }}
                          onRemovePrompt={(entryIndex) => {
                            const next = editDraft.prompts.filter((_, i) => i !== entryIndex);
                            setEditDraft({
                              ...editDraft,
                              prompts: next.length ? next : [{ body: "" }],
                            });
                          }}
                          onMovePrompt={(from, to) => {
                            setEditDraft({
                              ...editDraft,
                              prompts: moveArrayItem(editDraft.prompts, from, to),
                            });
                          }}
                        />
                      )}
                      <div className="edit-actions">
                        <button
                          className="button button-primary"
                          type="submit"
                          onPointerDown={(event) => event.preventDefault()}
                          onPointerUp={() => runSubmitOnce(() =>
                            handleSaveEdit(prompt.id, draftFromEditDom())
                          )}
                        >
                          {messages.manager.save}
                        </button>
                        <button
                          className="button button-secondary"
                          type="button"
                          onClick={() => setEditingId(null)}
                        >
                          {messages.manager.cancel}
                        </button>
                      </div>
                    </form>
                  ) : deleteConfirmId === prompt.id ? (
                    <div className="delete-confirm">
                      <span>{messages.manager.deleteConfirm}</span>
                      <div className="confirm-actions">
                        <button
                          className="button button-danger"
                          onClick={() => runSubmitOnce(async () => {
                            await onDelete(prompt.id);
                            setDeleteConfirmId(null);
                          })}
                        >
                          {messages.manager.confirm}
                        </button>
                        <button
                          className="button button-secondary"
                          onClick={() => setDeleteConfirmId(null)}
                        >
                          {messages.manager.cancel}
                        </button>
                      </div>
                    </div>
                  ) : (
                    <div className="prompt-content">
                      <div className="prompt-info">
                        <div className="prompt-title-row">
                          <strong>{prompt.title}</strong>
                          <PromptKindBadge prompt={prompt} messages={messages} />
                          <SendBehaviorBadge prompt={prompt} messages={messages} />
                        </div>
                        <span className="prompt-preview-lines">
                          {getPromptContainerPreviewLines(prompt).map((line) => (
                            <span className="prompt-preview-line" key={line}>
                              {line}
                            </span>
                          ))}
                        </span>
                      </div>
                      <div className="prompt-actions">
                        <button
                          className="button icon-button"
                          aria-label={messages.manager.moveCombinedPromptUp(prompt.title)}
                          onClick={() => handleMoveUp(index)}
                          disabled={reorderPending || index === 0}
                        >
                          ↑
                        </button>
                        <button
                          className="button icon-button"
                          aria-label={messages.manager.moveCombinedPromptDown(prompt.title)}
                          onClick={() => handleMoveDown(index)}
                          disabled={reorderPending || index === visiblePrompts.length - 1}
                        >
                          ↓
                        </button>
                        <button
                          className="button button-secondary"
                          onClick={() => {
                            setEditingId(prompt.id);
                            setEditDraft(draftFromPrompt(prompt));
                          }}
                        >
                          {messages.manager.edit}
                        </button>
                        <div
                          className="prompt-action-menu-wrap"
                          onPointerDown={(event) => event.stopPropagation()}
                        >
                          <button
                            className="button icon-button prompt-more-button"
                            type="button"
                            aria-label={messages.manager.moreActions(prompt.title)}
                            aria-haspopup="menu"
                            aria-expanded={openMenuId === prompt.id}
                            disabled={groupActionBusy}
                            onClick={() => setOpenMenuId(
                              openMenuId === prompt.id ? null : prompt.id
                            )}
                          >
                            ⋯
                          </button>
                          {openMenuId === prompt.id ? (
                            <div className="prompt-action-menu" role="menu">
                              <div className="prompt-action-menu-label" aria-hidden="true">
                                {messages.settings.clickBehaviorField}
                              </div>
                              {(["inherit", "paste_only", "paste_enter"] as const).map((value) => {
                                const current = prompt.sendBehavior === "paste_only"
                                  || prompt.sendBehavior === "paste_enter"
                                  ? prompt.sendBehavior
                                  : "inherit";
                                const selected = current === value;
                                const label = value === "inherit"
                                  ? messages.settings.followGlobal
                                  : value === "paste_only"
                                    ? messages.settings.pasteOnly
                                    : messages.settings.pasteEnter;
                                return (
                                  <button
                                    key={value}
                                    className={`prompt-action-menu-option${selected ? " is-selected" : ""}`}
                                    type="button"
                                    role="menuitemradio"
                                    aria-checked={selected}
                                    onClick={() => {
                                      setOpenMenuId(null);
                                      runSubmitOnce(() => onUpdate(prompt.id, { sendBehavior: value }));
                                    }}
                                  >
                                    <span className="prompt-action-menu-check" aria-hidden="true">
                                      {selected ? "✓" : ""}
                                    </span>
                                    {label}
                                  </button>
                                );
                              })}
                              <div className="prompt-action-menu-divider" aria-hidden="true" />
                              {prompt.type === "group" ? (
                                <button
                                  type="button"
                                  role="menuitem"
                                  onClick={() => {
                                    if (groupActionPendingRef.current) return;
                                    setSplitConfirmId(prompt.id);
                                    setOpenMenuId(null);
                                    setGroupActionError(null);
                                  }}
                                >
                                  {messages.manager.splitIntoSingles}
                                </button>
                              ) : null}
                              <button
                                className="is-danger"
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  setDeleteConfirmId(prompt.id);
                                  setOpenMenuId(null);
                                }}
                              >
                                {messages.manager.delete}
                              </button>
                            </div>
                          ) : null}
                        </div>
                      </div>
                    </div>
                  )}
                </PromptReorderRow>
              ))}
            </Reorder.Group>
            </div>
            {mergeIds.length > 0 ? (
              <div
                className="manager-dialog-backdrop"
                onMouseDown={(event) => {
                  if (event.target === event.currentTarget) closeMergeDialog();
                }}
              >
                <form
                  className="manager-dialog merge-dialog"
                  role="dialog"
                  aria-modal="true"
                  aria-busy={groupActionBusy}
                  aria-labelledby="merge-dialog-title"
                  onKeyDown={(event) => {
                    if (event.key === "Escape") closeMergeDialog();
                  }}
                  onSubmit={(event) => {
                    event.preventDefault();
                    void handleCombine();
                  }}
                >
                  <div>
                    <h2 id="merge-dialog-title">{messages.manager.combineDialogTitle}</h2>
                    <p>{messages.manager.combineDialogDescription}</p>
                  </div>
                  <label className="manager-dialog-field">
                    <span>{messages.manager.groupName}</span>
                    <input
                      ref={mergeTitleInputRef}
                      className="field"
                      disabled={groupActionBusy}
                      value={mergeTitle}
                      onChange={(event) => {
                        if (!groupActionPendingRef.current) setMergeTitle(event.target.value);
                      }}
                    />
                  </label>
                  <div className="merge-order-section">
                    <strong>{messages.manager.executionOrder}</strong>
                    <div className="merge-order-list">
                      {mergePrompts.map((prompt, index) => (
                        <div
                          className={`merge-order-item ${draggingMergeId === prompt.id ? "is-dragging" : ""}`}
                          draggable={!groupActionBusy}
                          key={prompt.id}
                          onDragStart={(event) => {
                            if (groupActionPendingRef.current) {
                              event.preventDefault();
                              return;
                            }
                            setDraggingMergeId(prompt.id);
                            event.dataTransfer.effectAllowed = "move";
                            event.dataTransfer.setData("text/plain", prompt.id);
                          }}
                          onDragEnd={() => setDraggingMergeId(null)}
                          onDragOver={(event) => event.preventDefault()}
                          onDrop={(event) => {
                            event.preventDefault();
                            if (groupActionPendingRef.current) return;
                            const sourceId = draggingMergeId
                              ?? event.dataTransfer.getData("text/plain");
                            moveMergeItem(sourceId, prompt.id);
                            setDraggingMergeId(null);
                          }}
                        >
                          <span className="merge-order-handle" aria-hidden="true">⋮⋮</span>
                          <span className="merge-order-number">{index + 1}</span>
                          <span className="merge-order-copy">
                            <strong>{prompt.title}</strong>
                            <small>{getPromptContainerPreviewLines(prompt)[0]}</small>
                          </span>
                          <span className="merge-order-actions">
                            <button
                              type="button"
                              aria-label={messages.manager.moveCombinedPromptUp(prompt.title)}
                              disabled={groupActionBusy || index === 0}
                              onClick={() => moveMergeItemBy(index, -1)}
                            >
                              ↑
                            </button>
                            <button
                              type="button"
                              aria-label={messages.manager.moveCombinedPromptDown(prompt.title)}
                              disabled={groupActionBusy || index === mergePrompts.length - 1}
                              onClick={() => moveMergeItemBy(index, 1)}
                            >
                              ↓
                            </button>
                            <button
                              type="button"
                              aria-label={messages.manager.removeFromCombination(prompt.title)}
                              disabled={groupActionBusy}
                              onClick={() => {
                                if (groupActionPendingRef.current) return;
                                setMergeIds((ids) => ids.filter((id) => id !== prompt.id));
                              }}
                            >
                              ×
                            </button>
                          </span>
                        </div>
                      ))}
                    </div>
                  </div>
                  <label className="merge-delete-originals">
                    <input
                      type="checkbox"
                      disabled={groupActionBusy}
                      checked={deleteOriginals}
                      onChange={(event) => {
                        if (!groupActionPendingRef.current) {
                          setDeleteOriginals(event.target.checked);
                        }
                      }}
                    />
                    <span>
                      <strong>{messages.manager.deleteOriginals(mergeIds.length)}</strong>
                      <small>{messages.manager.deleteOriginalsDescription}</small>
                    </span>
                  </label>
                  {groupActionError ? (
                    <div className="manager-dialog-error" role="alert">
                      {groupActionError}
                    </div>
                  ) : null}
                  <div className="manager-dialog-actions">
                    <button
                      className="button button-secondary"
                      type="button"
                      disabled={groupActionBusy}
                      onClick={closeMergeDialog}
                    >
                      {messages.manager.cancel}
                    </button>
                    <button
                      className="button button-primary"
                      type="submit"
                      disabled={groupActionBusy
                        || mergeIds.length < 2
                        || !mergeTitle.trim()}
                    >
                      {deleteOriginals
                        ? messages.manager.createGroupAndDelete
                        : messages.manager.createGroup}
                    </button>
                  </div>
                </form>
              </div>
            ) : null}
            {splitPrompt ? (
              <div
                className="manager-dialog-backdrop"
                onMouseDown={(event) => {
                  if (event.target === event.currentTarget) closeSplitDialog();
                }}
              >
                <div
                  className="manager-dialog split-dialog"
                  role="dialog"
                  aria-modal="true"
                  aria-busy={groupActionBusy}
                  aria-labelledby="split-dialog-title"
                  onKeyDown={(event) => {
                    if (event.key === "Escape") closeSplitDialog();
                  }}
                >
                  <div>
                    <h2 id="split-dialog-title">{messages.manager.splitDialogTitle}</h2>
                    <p>{messages.manager.splitDialogDescription(
                      splitPrompt.title,
                      getPromptContainerBodies(splitPrompt).length
                    )}</p>
                  </div>
                  <div className="split-preview-list">
                    {[...splitPrompt.prompts]
                      .sort((a, b) => a.order - b.order)
                      .map((entry, index) => (
                        <div key={entry.id}>
                          <span>{index + 1}</span>
                          <strong>{entry.title || `${splitPrompt.title} ${index + 1}`}</strong>
                        </div>
                      ))}
                  </div>
                  {groupActionError ? (
                    <div className="manager-dialog-error" role="alert">
                      {groupActionError}
                    </div>
                  ) : null}
                  <div className="manager-dialog-actions">
                    <button
                      className="button button-secondary"
                      type="button"
                      autoFocus
                      disabled={groupActionBusy}
                      onClick={closeSplitDialog}
                    >
                      {messages.manager.cancel}
                    </button>
                    <button
                      className="button button-primary"
                      type="button"
                      disabled={groupActionBusy}
                      onClick={() => void handleSplit(splitPrompt.id)}
                    >
                      {messages.manager.confirmSplit}
                    </button>
                  </div>
                </div>
              </div>
            ) : null}
          </section>
        </div>
      </div>
    </div>
  );
}

interface GroupFieldsProps {
  prompts: DraftPrompt[];
  intervalMs: number;
  messages: Messages;
  promptRef?: (index: number, node: HTMLTextAreaElement | null) => void;
  onIntervalChange: (intervalMs: number) => void;
  onPromptChange: (index: number, value: string) => void;
  onInsertPrompt: (index: number) => void;
  onRemovePrompt: (index: number) => void;
  onMovePrompt: (from: number, to: number) => void;
}

function GroupFields({
  prompts,
  intervalMs,
  messages,
  promptRef,
  onIntervalChange,
  onPromptChange,
  onInsertPrompt,
  onRemovePrompt,
  onMovePrompt,
}: GroupFieldsProps) {
  const [draggingIndex, setDraggingIndex] = useState<number | null>(null);
  const [intervalSecondsInput, setIntervalSecondsInput] = useState(() =>
    formatIntervalSeconds(intervalMs)
  );

  useEffect(() => {
    setIntervalSecondsInput(formatIntervalSeconds(intervalMs));
  }, [intervalMs]);

  const handleDragStart = (event: DragEvent<HTMLButtonElement>, index: number) => {
    setDraggingIndex(index);
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", String(index));
  };

  const handleDrop = (event: DragEvent<HTMLDivElement>, targetIndex: number) => {
    event.preventDefault();
    const sourceIndex = draggingIndex;
    setDraggingIndex(null);
    if (sourceIndex === null || sourceIndex === targetIndex) return;
    onMovePrompt(sourceIndex, targetIndex);
  };

  const handleIntervalChange = (value: string) => {
    setIntervalSecondsInput(value);
    if (!value.trim()) return;
    onIntervalChange(intervalSecondsToMs(Number(value)));
  };

  return (
    <div className="group-editor">
      <label className="interval-field">
        <span>{messages.manager.delayBetweenPrompts}</span>
        <input
          aria-label={messages.manager.delayBetweenPrompts}
          className="field"
          type="number"
          min={MIN_GROUP_INTERVAL_MS / 1000}
          max={MAX_GROUP_INTERVAL_MS / 1000}
          step={GROUP_INTERVAL_STEP_SECONDS}
          value={intervalSecondsInput}
          onBlur={() => {
            if (!intervalSecondsInput.trim()) {
              setIntervalSecondsInput(formatIntervalSeconds(intervalMs));
            }
          }}
          onChange={(e) => handleIntervalChange(e.target.value)}
        />
        <span>s</span>
      </label>
      <div className="group-prompt-list">
        {prompts.map((prompt, index) => (
          <div
            className={`group-prompt-row ${draggingIndex === index ? "is-dragging" : ""}`}
            key={index}
            onDragOver={(event) => event.preventDefault()}
            onDrop={(event) => handleDrop(event, index)}
          >
            <button
              aria-label={messages.manager.dragPrompt(index + 1)}
              className="group-prompt-handle"
              draggable
              type="button"
              onDragStart={(event) => handleDragStart(event, index)}
              onDragEnd={() => setDraggingIndex(null)}
            >
              <span aria-hidden="true">⋮⋮</span>
              {messages.manager.promptLabel(index + 1)}
            </button>
            <textarea
              ref={(node) => promptRef?.(index, node)}
              aria-label={messages.manager.promptBody(index + 1)}
              className="field prompt-body-field"
              value={prompt.body}
              onChange={(e) => onPromptChange(index, e.target.value)}
            />
            <div className="group-prompt-actions">
              <button
                aria-label={messages.manager.insertPromptAfter(index + 1)}
                className="button icon-button group-icon-button"
                type="button"
                onClick={() => onInsertPrompt(index)}
              >
                +
              </button>
              <button
                aria-label={messages.manager.removePrompt(index + 1)}
                className="button icon-button group-icon-button"
                type="button"
                disabled={prompts.length === 1}
                onClick={() => onRemovePrompt(index)}
              >
                -
              </button>
              <button
                aria-label={messages.manager.movePromptUp(index + 1)}
                className="button icon-button group-icon-button"
                type="button"
                disabled={index === 0}
                onClick={() => onMovePrompt(index, index - 1)}
              >
                ↑
              </button>
              <button
                aria-label={messages.manager.movePromptDown(index + 1)}
                className="button icon-button group-icon-button"
                type="button"
                disabled={index === prompts.length - 1}
                onClick={() => onMovePrompt(index, index + 1)}
              >
                ↓
              </button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
