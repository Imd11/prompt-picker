import { useEffect, useRef, useState } from "react";
import type { PromptCategory, PromptContainer } from "../shared/promptTypes";
import {
  getPromptContainerBodies,
  getPromptContainerPreviewLines,
} from "../shared/promptTypes";
import type { Messages } from "../shared/i18n";

interface PromptQuickListProps {
  prompts: PromptContainer[];
  categories?: PromptCategory[];
  activeCategoryId?: string | null;
  getCategoryDisplayName?: (category: PromptCategory) => string;
  onSelectCategory?: (categoryId: string) => void;
  messages: Messages["quickList"];
  groupMeta: Messages["manager"]["groupMeta"];
  onSelect: (prompt: PromptContainer) => void;
  submittingPromptId?: string | null;
  hoverResetKey?: number;
  nativePointerPosition?: NativePopoverPointerPosition | null;
  onGroupPreview?: () => void;
}

export type NativePopoverPointerPosition = {
  x: number;
  y: number;
  inside: boolean;
};

type HoverPreviewState = {
  promptId: string;
  left: number;
  top: number;
  width: number;
  placement: "above" | "below";
};

type HoverPreviewAnchor = {
  prompt: PromptContainer;
  target: HTMLElement;
};

const HOVER_PREVIEW_MIN_USEFUL_SPACE = 120;
const HOVER_PREVIEW_GAP = 8;
const HOVER_PREVIEW_MARGIN = 10;
const HOVER_PREVIEW_DELAY_MS = 1500;
function clamp(value: number, min: number, max: number): number {
  if (max < min) return min;
  return Math.min(Math.max(value, min), max);
}

export function PromptQuickList({
  prompts,
  categories,
  activeCategoryId = null,
  getCategoryDisplayName,
  onSelectCategory,
  messages,
  groupMeta,
  onSelect,
  submittingPromptId = null,
  hoverResetKey = 0,
  nativePointerPosition = null,
  onGroupPreview,
}: PromptQuickListProps) {
  const [hoveredPromptId, setHoveredPromptId] = useState<string | null>(null);
  const [hoverPreview, setHoverPreview] = useState<HoverPreviewState | null>(null);
  const listRef = useRef<HTMLDivElement | null>(null);
  const hoverPreviewTimerRef = useRef<number | null>(null);
  const hoverPreviewAnchorRef = useRef<HoverPreviewAnchor | null>(null);
  const livePointerPositionRef = useRef<NativePopoverPointerPosition | null>(null);
  const hoveredPrompt = prompts.find((prompt) => prompt.id === hoverPreview?.promptId) ?? null;

  useEffect(() => {
    return () => {
      clearHoverPreviewTimer();
    };
  }, []);

  useEffect(() => {
    livePointerPositionRef.current = null;
    hidePromptHover();
    const focusedElement = document.activeElement;
    if (
      focusedElement instanceof HTMLElement
      && listRef.current?.contains(focusedElement)
    ) {
      focusedElement.blur();
    }
  }, [hoverResetKey]);

  useEffect(() => {
    if (!nativePointerPosition) return;
    livePointerPositionRef.current = nativePointerPosition.inside
      ? nativePointerPosition
      : null;
    reconcilePointerPosition(nativePointerPosition);
  }, [nativePointerPosition, prompts]);

  function reconcilePointerPosition(
    pointerPosition: NativePopoverPointerPosition,
    showDelayedPreview = true
  ) {
    if (!pointerPosition.inside) {
      hidePromptHover();
      return;
    }

    const pointedElement = document.elementFromPoint?.(
      pointerPosition.x,
      pointerPosition.y
    );
    const option = pointedElement?.closest<HTMLButtonElement>(".prompt-quick-item");
    if (!option || !listRef.current?.contains(option) || option.disabled) {
      hidePromptHover();
      return;
    }

    const prompt = prompts.find((candidate) => candidate.id === option.dataset.promptId);
    if (!prompt) {
      hidePromptHover();
      return;
    }

    setHoveredPromptId(prompt.id);
    if (showDelayedPreview) {
      scheduleHoverPreview(prompt, option);
    }
  }

  function clearHoverPreviewTimer() {
    if (hoverPreviewTimerRef.current !== null) {
      window.clearTimeout(hoverPreviewTimerRef.current);
      hoverPreviewTimerRef.current = null;
    }
  }

  function createHoverPreviewState(
    prompt: PromptContainer,
    target: HTMLElement
  ): HoverPreviewState {
    const shell = target.closest(".prompt-quick-shell") as HTMLElement | null;
    const shellRect = shell?.getBoundingClientRect();
    const targetRect = target.getBoundingClientRect();
    const shellWidth = Math.max(
      shellRect?.width ?? 0,
      shell?.clientWidth ?? 0,
      targetRect.width + (HOVER_PREVIEW_MARGIN * 2)
    );
    const shellHeight = Math.max(
      shellRect?.height ?? 0,
      shell?.clientHeight ?? 0,
      320
    );
    const localLeft = shellRect ? targetRect.left - shellRect.left : target.offsetLeft;
    const localTop = shellRect ? targetRect.top - shellRect.top : target.offsetTop;
    const width = Math.min(
      targetRect.width,
      shellWidth - (HOVER_PREVIEW_MARGIN * 2)
    );
    const maxLeft = Math.max(
      HOVER_PREVIEW_MARGIN,
      shellWidth - width - HOVER_PREVIEW_MARGIN
    );
    const targetBottom = localTop + targetRect.height;
    const availableBelow = shellHeight - targetBottom - HOVER_PREVIEW_GAP - HOVER_PREVIEW_MARGIN;
    const availableAbove = localTop - HOVER_PREVIEW_GAP - HOVER_PREVIEW_MARGIN;
    const placement =
      availableAbove >= HOVER_PREVIEW_MIN_USEFUL_SPACE || availableAbove >= availableBelow
        ? "above"
        : "below";
    const top = placement === "above"
      ? Math.max(HOVER_PREVIEW_MARGIN, localTop - HOVER_PREVIEW_GAP)
      : Math.min(
        targetBottom + HOVER_PREVIEW_GAP,
        Math.max(HOVER_PREVIEW_MARGIN, shellHeight - HOVER_PREVIEW_MARGIN)
      );

    return {
      promptId: prompt.id,
      left: clamp(localLeft, HOVER_PREVIEW_MARGIN, maxLeft),
      top,
      width,
      placement,
    };
  }

  function scheduleHoverPreview(
    prompt: PromptContainer,
    target: HTMLElement
  ) {
    const currentAnchor = hoverPreviewAnchorRef.current;
    if (
      currentAnchor?.prompt.id === prompt.id &&
      currentAnchor.target === target &&
      (hoverPreviewTimerRef.current !== null || hoverPreview?.promptId === prompt.id)
    ) {
      return;
    }

    clearHoverPreviewTimer();
    hoverPreviewAnchorRef.current = {
      prompt,
      target,
    };
    hoverPreviewTimerRef.current = window.setTimeout(() => {
      hoverPreviewTimerRef.current = null;
      const anchor = hoverPreviewAnchorRef.current;
      if (!anchor || !document.contains(anchor.target)) return;
      setHoverPreview(createHoverPreviewState(
        anchor.prompt,
        anchor.target
      ));
    }, HOVER_PREVIEW_DELAY_MS);
  }

  function hideHoverPreview() {
    clearHoverPreviewTimer();
    hoverPreviewAnchorRef.current = null;
    setHoverPreview(null);
  }

  function showPromptHover(prompt: PromptContainer) {
    reportGroupPreview(prompt);
  }

  function hidePromptHover() {
    setHoveredPromptId(null);
    hideHoverPreview();
  }

  function leavePromptHover() {
    livePointerPositionRef.current = null;
    hidePromptHover();
  }

  function handleListScroll() {
    hideHoverPreview();
    const pointerPosition = livePointerPositionRef.current;
    if (!pointerPosition) {
      setHoveredPromptId(null);
      return;
    }
    reconcilePointerPosition(pointerPosition, false);
  }

  function selectPrompt(prompt: PromptContainer) {
    hidePromptHover();
    onSelect(prompt);
  }

  function reportGroupPreview(prompt: PromptContainer) {
    if (prompt.type === "group") {
      onGroupPreview?.();
    }
  }

  return (
    <div className="prompt-quick-shell">
      {categories && categories.length > 1 ? (
        <div className="prompt-category-tabs" role="tablist" aria-label={messages.categoriesLabel}>
          {categories.map((category) => (
            <button
              key={category.id}
              className={`prompt-category-tab ${category.id === activeCategoryId ? "is-active" : ""}`}
              type="button"
              role="tab"
              aria-selected={category.id === activeCategoryId}
              onClick={() => onSelectCategory?.(category.id)}
            >
              {getCategoryDisplayName?.(category) ?? category.name}
            </button>
          ))}
        </div>
      ) : null}
      <div
        ref={listRef}
        className="prompt-quick-list"
        role="listbox"
        aria-label={messages.ariaLabel}
        onScroll={handleListScroll}
      >
        {prompts.length === 0 ? (
          <div className="prompt-quick-empty">
            <strong>
              {categories || activeCategoryId
                ? messages.noPromptsInCategoryTitle
                : messages.noPromptsTitle}
            </strong>
            <span>
              {categories || activeCategoryId
                ? messages.noPromptsInCategoryDescription
                : messages.noPromptsDescription}
            </span>
          </div>
        ) : (
          prompts.map((prompt) => (
            <button
              key={prompt.id}
              data-prompt-id={prompt.id}
              className={`prompt-quick-item ${
                prompt.type === "group" ? "prompt-quick-item-group" : ""
              } ${prompt.id === hoveredPromptId ? "is-hovered" : ""}`}
              type="button"
              role="option"
              aria-selected="false"
              disabled={submittingPromptId === prompt.id}
              onPointerEnter={() => showPromptHover(prompt)}
              onPointerMove={(event) => {
                livePointerPositionRef.current = {
                  x: event.clientX,
                  y: event.clientY,
                  inside: true,
                };
                setHoveredPromptId(prompt.id);
                scheduleHoverPreview(prompt, event.currentTarget);
              }}
              onPointerLeave={leavePromptHover}
              onPointerCancel={leavePromptHover}
              onFocus={() => reportGroupPreview(prompt)}
              onBlur={hideHoverPreview}
              onClick={() => selectPrompt(prompt)}
            >
              <span className="prompt-quick-title-row">
                <span className="prompt-quick-title" title={prompt.title}>
                  {prompt.title}
                </span>
                {prompt.type === "group" ? (
                  <span className="prompt-quick-meta">
                    {groupMeta(getPromptContainerBodies(prompt).length)}
                  </span>
                ) : null}
              </span>
              <span className="prompt-quick-preview-lines">
                {getPromptContainerPreviewLines(prompt).map((line) => (
                  <span className="prompt-quick-preview-line" key={line}>
                    {line}
                  </span>
                ))}
              </span>
            </button>
          ))
        )}
      </div>
      {hoveredPrompt && hoverPreview ? (
        <PromptHoverPreview
          prompt={hoveredPrompt}
          left={hoverPreview.left}
          top={hoverPreview.top}
          width={hoverPreview.width}
          placement={hoverPreview.placement}
        />
      ) : null}
    </div>
  );
}

function PromptHoverPreview({
  prompt,
  left,
  top,
  width,
  placement,
}: {
  prompt: PromptContainer;
  left: number;
  top: number;
  width: number;
  placement: "above" | "below";
}) {
  const bodies = getPromptContainerBodies(prompt);

  return (
    <aside
      className={`prompt-hover-preview prompt-hover-preview-floating ${
        placement === "above" ? "is-above" : "is-below"
      }`}
      role="tooltip"
      style={{ left, top, width }}
    >
      <div className="prompt-hover-preview-body">
        {prompt.type === "group" ? (
          bodies.map((body, index) => (
            <p key={`${index}-${body}`}>
              {index + 1}. {body}
            </p>
          ))
        ) : (
          <p>{bodies[0] ?? ""}</p>
        )}
      </div>
    </aside>
  );
}
