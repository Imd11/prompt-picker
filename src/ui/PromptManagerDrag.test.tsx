import type { HTMLAttributes, ReactNode } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getMessages } from "../shared/i18n";
import type { PromptCategory, PromptContainer } from "../shared/promptTypes";

const motionMocks = vi.hoisted(() => ({
  start: vi.fn(),
  onReorder: null as ((ids: string[]) => void) | null,
  dragEndById: new Map<string, () => void>(),
}));

vi.mock("motion/react", () => ({
  useDragControls: () => ({ start: motionMocks.start }),
  Reorder: {
    Group: ({
      children,
      className,
      role,
      onReorder,
      "data-reorder-list": reorderList,
    }: {
      children?: ReactNode;
      className?: string;
      role?: string;
      onReorder?: (ids: string[]) => void;
      "data-reorder-list"?: string;
    }) => {
      motionMocks.onReorder = onReorder ?? null;
      return (
        <div className={className} role={role} data-reorder-list={reorderList}>
          {children}
        </div>
      );
    },
    Item: ({
      children,
      className,
      role,
      value,
      layout,
      onPointerDown,
      onDragEnd,
      "data-reorder-id": reorderId,
    }: {
      children?: ReactNode;
      className?: string;
      role?: string;
      value?: string;
      layout?: boolean | "position" | "size" | "preserve-aspect";
      onPointerDown?: HTMLAttributes<HTMLDivElement>["onPointerDown"];
      onDragEnd?: () => void;
      "data-reorder-id"?: string;
    }) => {
      if (value && onDragEnd) motionMocks.dragEndById.set(value, onDragEnd);
      return (
        <div
          className={className}
          role={role}
          data-reorder-id={reorderId}
          data-layout={String(layout)}
          onPointerDown={onPointerDown}
        >
          {children}
        </div>
      );
    },
  },
}));

import { PromptManager } from "./PromptManager";

describe("prompt manager controlled dragging", () => {
  const category: PromptCategory = {
    id: "category-default",
    name: "Default",
    order: 0,
    createdAt: "2026-05-26T00:00:00.000Z",
    updatedAt: "2026-05-26T00:00:00.000Z",
  };

  const prompts: PromptContainer[] = [
    {
      id: "prompt-1",
      categoryId: category.id,
      title: "Code Review",
      type: "single",
      sendBehavior: "inherit",
      prompts: [{ id: "entry-1", body: "Review this code.", order: 0 }],
      intervalMs: 700,
      order: 0,
      createdAt: "2026-05-26T00:00:00.000Z",
      updatedAt: "2026-05-26T00:00:00.000Z",
    },
    {
      id: "prompt-2",
      categoryId: category.id,
      title: "Repair",
      type: "single",
      sendBehavior: "inherit",
      prompts: [{ id: "entry-2", body: "Repair the issue.", order: 0 }],
      intervalMs: 700,
      order: 1,
      createdAt: "2026-05-26T00:00:00.000Z",
      updatedAt: "2026-05-26T00:00:00.000Z",
    },
  ];

  beforeEach(() => {
    motionMocks.start.mockClear();
    motionMocks.onReorder = null;
    motionMocks.dragEndById.clear();
  });

  function renderManager(onReorder: (ids: string[]) => void | Promise<void> = () => {}) {
    render(
      <PromptManager
        prompts={prompts}
        categories={[category]}
        activeCategoryId={category.id}
        categoryCounts={{ [category.id]: prompts.length }}
        totalPromptCount={prompts.length}
        messages={getMessages("zh-CN")}
        onOpenSettings={() => {}}
        onSelectCategory={() => {}}
        onCreateCategory={() => {}}
        onRenameCategory={() => {}}
        onDeleteCategory={() => {}}
        getCategoryDisplayName={(item) => item.name}
        onCreate={() => {}}
        onCreateGroup={() => {}}
        onCombineSingles={() => {}}
        onSplitGroup={() => {}}
        onUpdate={() => {}}
        onDelete={() => {}}
        onReorder={onReorder}
        onImport={() => {}}
        onExport={() => {}}
      />
    );
  }

  it("starts dragging from row content but not from row actions", () => {
    renderManager();

    fireEvent.pointerDown(screen.getByText("Code Review"));
    expect(motionMocks.start).toHaveBeenCalledTimes(1);

    fireEvent.pointerDown(screen.getAllByRole("button", { name: "编辑" })[0]);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Code Review 的更多操作" }));
    fireEvent.pointerDown(screen.getByRole("button", { name: "下移 Code Review" }));

    expect(motionMocks.start).toHaveBeenCalledTimes(1);
  });

  it("animates reordered positions without scaling prompt row dimensions", () => {
    renderManager();

    const rows = screen.getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    rows.forEach((row) => {
      expect(row.getAttribute("data-layout")).toBe("position");
    });
  });

  it("persists the final Motion order when dragging ends", async () => {
    const onReorder = vi.fn();
    renderManager(onReorder);

    act(() => {
      motionMocks.onReorder?.(["prompt-2", "prompt-1"]);
    });
    act(() => {
      motionMocks.dragEndById.get("prompt-1")?.();
    });

    await waitFor(() => {
      expect(onReorder).toHaveBeenCalledWith(["prompt-2", "prompt-1"]);
    });
  });
});
