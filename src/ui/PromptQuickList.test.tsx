import { afterEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentProps } from "react";
import { getMessages } from "../shared/i18n";
import type { PromptContainer } from "../shared/promptTypes";
import { PromptQuickList } from "./PromptQuickList";

const prompts: PromptContainer[] = [
  {
    id: "1",
    categoryId: "category-default",
    title: "讨论方案",
    type: "single",
    sendBehavior: "inherit",
    prompts: [
      {
        id: "1-entry",
        body: "使用 brainstorming skill，先和我讨论方案，不要修改代码。",
        order: 0,
      },
    ],
    intervalMs: 700,
    order: 0,
    createdAt: "2026-05-26T00:00:00.000Z",
    updatedAt: "2026-05-26T00:00:00.000Z",
  },
  {
    id: "2",
    categoryId: "category-default",
    title: "修复流程",
    type: "group",
    sendBehavior: "inherit",
    prompts: [
      { id: "2-entry-1", body: "分析根本原因。", order: 0 },
      { id: "2-entry-2", body: "执行修复。", order: 1 },
      { id: "2-entry-3", body: "完成验证。", order: 2 },
    ],
    intervalMs: 700,
    order: 1,
    createdAt: "2026-05-26T00:00:00.000Z",
    updatedAt: "2026-05-26T00:00:00.000Z",
  }
];

function makeRect(
  left: number,
  top: number,
  width: number,
  height: number
): DOMRect {
  return {
    x: left,
    y: top,
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    toJSON: () => ({}),
  } as DOMRect;
}

function revealHoverPreview(
  option: HTMLElement
) {
  fireEvent.pointerMove(option);
  act(() => {
    vi.advanceTimersByTime(1500);
  });
}

function renderQuickList(
  props: Partial<ComponentProps<typeof PromptQuickList>> = {}
) {
  const zh = getMessages("zh-CN");
  return render(
    <PromptQuickList
      prompts={prompts}
      messages={zh.quickList}
      groupMeta={zh.manager.groupMeta}
      onSelect={() => {}}
      {...props}
    />
  );
}

describe("PromptQuickList", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("renders compact prompt container options", () => {
    renderQuickList();

    expect(screen.getByText("讨论方案")).toBeTruthy();
    expect(screen.getByText(/brainstorming skill/)).toBeTruthy();
    expect(screen.queryByText("Single · 1 prompt")).toBeNull();
  });

  it("starts unhighlighted and follows the live pointer position", () => {
    renderQuickList();

    const first = screen.getByRole("option", { name: /讨论方案/i });
    const second = screen.getByRole("option", { name: /修复流程/i });
    expect(first.classList.contains("is-hovered")).toBe(false);
    expect(second.classList.contains("is-hovered")).toBe(false);

    fireEvent.pointerEnter(first);
    expect(first.classList.contains("is-hovered")).toBe(false);

    fireEvent.pointerMove(first);
    expect(first.classList.contains("is-hovered")).toBe(true);
    expect(second.classList.contains("is-hovered")).toBe(false);

    fireEvent.pointerMove(second);
    expect(first.classList.contains("is-hovered")).toBe(false);
    expect(second.classList.contains("is-hovered")).toBe(true);

    fireEvent.pointerLeave(second);
    expect(second.classList.contains("is-hovered")).toBe(false);
  });

  it("follows native pointer coordinates while the popover remains non-activating", () => {
    const elementFromPoint = vi.fn<() => Element | null>().mockReturnValue(null);
    Object.defineProperty(document, "elementFromPoint", {
      configurable: true,
      value: elementFromPoint,
    });
    const { rerender } = renderQuickList();
    const first = screen.getByRole("option", { name: /讨论方案/i });
    const second = screen.getByRole("option", { name: /修复流程/i });
    const zh = getMessages("zh-CN");

    elementFromPoint.mockReturnValue(first);
    rerender(
      <PromptQuickList
        prompts={prompts}
        messages={zh.quickList}
        groupMeta={zh.manager.groupMeta}
        onSelect={() => {}}
        nativePointerPosition={{ x: 40, y: 80, inside: true }}
      />
    );
    expect(first.classList.contains("is-hovered")).toBe(true);
    expect(second.classList.contains("is-hovered")).toBe(false);

    elementFromPoint.mockReturnValue(second);
    rerender(
      <PromptQuickList
        prompts={prompts}
        messages={zh.quickList}
        groupMeta={zh.manager.groupMeta}
        onSelect={() => {}}
        nativePointerPosition={{ x: 40, y: 180, inside: true }}
      />
    );
    expect(first.classList.contains("is-hovered")).toBe(false);
    expect(second.classList.contains("is-hovered")).toBe(true);

    rerender(
      <PromptQuickList
        prompts={prompts}
        messages={zh.quickList}
        groupMeta={zh.manager.groupMeta}
        onSelect={() => {}}
        nativePointerPosition={{ x: 0, y: 0, inside: false }}
      />
    );
    expect(first.classList.contains("is-hovered")).toBe(false);
    expect(second.classList.contains("is-hovered")).toBe(false);

    Reflect.deleteProperty(document, "elementFromPoint");
  });

  it("realigns the highlight after scrolling under a stationary native pointer", () => {
    const elementFromPoint = vi.fn<() => Element | null>().mockReturnValue(null);
    Object.defineProperty(document, "elementFromPoint", {
      configurable: true,
      value: elementFromPoint,
    });
    const { rerender } = renderQuickList();
    const first = screen.getByRole("option", { name: /讨论方案/i });
    const second = screen.getByRole("option", { name: /修复流程/i });
    const zh = getMessages("zh-CN");

    try {
      elementFromPoint.mockReturnValue(first);
      rerender(
        <PromptQuickList
          prompts={prompts}
          messages={zh.quickList}
          groupMeta={zh.manager.groupMeta}
          onSelect={() => {}}
          nativePointerPosition={{ x: 40, y: 80, inside: true }}
        />
      );
      expect(first.classList.contains("is-hovered")).toBe(true);

      elementFromPoint.mockReturnValue(second);
      fireEvent.scroll(screen.getByRole("listbox", { name: "提示词" }));

      expect(first.classList.contains("is-hovered")).toBe(false);
      expect(second.classList.contains("is-hovered")).toBe(true);
    } finally {
      Reflect.deleteProperty(document, "elementFromPoint");
    }
  });

  it("clears stale pointer highlighting and focus when a reused popover resets", () => {
    const { rerender } = renderQuickList({ hoverResetKey: 0 });
    const option = screen.getByRole("option", { name: /讨论方案/i });
    fireEvent.pointerMove(option);
    option.focus();
    expect(option.classList.contains("is-hovered")).toBe(true);
    expect(document.activeElement).toBe(option);

    const zh = getMessages("zh-CN");
    rerender(
      <PromptQuickList
        prompts={prompts}
        messages={zh.quickList}
        groupMeta={zh.manager.groupMeta}
        onSelect={() => {}}
        hoverResetKey={1}
      />
    );

    expect(option.classList.contains("is-hovered")).toBe(false);
    expect(document.activeElement).not.toBe(option);
  });

  it("renders category tabs and switches active category", () => {
    const onSelectCategory = vi.fn();
    renderQuickList({
      categories: [
        { id: "cat-dev", name: "开发代码", order: 0, createdAt: "", updatedAt: "" },
        { id: "cat-writing", name: "写作", order: 1, createdAt: "", updatedAt: "" },
      ],
      activeCategoryId: "cat-dev",
      onSelectCategory,
    });

    expect(screen.getByRole("tab", { name: "开发代码" }).getAttribute("aria-selected"))
      .toBe("true");
    fireEvent.click(screen.getByRole("tab", { name: "写作" }));
    expect(onSelectCategory).toHaveBeenCalledWith("cat-writing");
  });

  it("renders category tabs before the scrollable prompt list", () => {
    renderQuickList({
      categories: [
        { id: "cat-dev", name: "开发代码", order: 0, createdAt: "", updatedAt: "" },
        { id: "cat-writing", name: "写作", order: 1, createdAt: "", updatedAt: "" },
      ],
      activeCategoryId: "cat-dev",
    });

    const shell = document.querySelector(".prompt-quick-shell");
    const tabs = document.querySelector(".prompt-category-tabs");
    const list = document.querySelector(".prompt-quick-list");

    expect(shell?.firstElementChild).toBe(tabs);
    expect(tabs?.nextElementSibling).toBe(list);
  });

  it("shows category-aware empty state in quick picker", () => {
    renderQuickList({
      prompts: [],
      categories: [{ id: "cat-writing", name: "写作", order: 0, createdAt: "", updatedAt: "" }],
      activeCategoryId: "cat-writing",
    });

    expect(screen.getByText("这个分类还没有提示词")).toBeTruthy();
  });

  it("renders group containers with two one-line preview entries", () => {
    renderQuickList();

    expect(screen.getByText("修复流程")).toBeTruthy();
    expect(screen.getByText("群组 · 3 条")).toBeTruthy();
    expect(screen.queryByText(/700ms/)).toBeNull();
    const groupOption = screen.getByRole("option", { name: /修复流程/i });
    expect(groupOption.textContent).toContain("1. 分析根本原因。");
    expect(groupOption.textContent).toContain("2. 执行修复。");
    expect(groupOption.textContent).not.toContain("3. 完成验证。");
    expect(screen.getByText("1. 分析根本原因。")).toBeTruthy();
    expect(screen.getByText("2. 执行修复。")).toBeTruthy();
    expect(screen.queryByText("3. 完成验证。")).toBeNull();
  });

  it("keeps long English group titles and metadata in the compact three-row card", () => {
    const en = getMessages("en-US");
    const title = "End-to-End Debugging & Fix Workflow";
    const longEnglishGroup: PromptContainer = {
      ...prompts[1],
      title,
      prompts: Array.from({ length: 8 }, (_, index) => ({
        id: `long-group-entry-${index + 1}`,
        body: `Step ${index + 1} prompt body.`,
        order: index,
      })),
    };

    renderQuickList({
      prompts: [longEnglishGroup],
      messages: en.quickList,
      groupMeta: en.manager.groupMeta,
    });

    const groupOption = screen.getByRole("option", { name: new RegExp(title) });
    expect(groupOption).toBeTruthy();
    expect(screen.getByTitle(title)).toBeTruthy();
    expect(screen.getByText("Group · 8 prompts")).toBeTruthy();
    expect(screen.getByText("1. Step 1 prompt body.")).toBeTruthy();
    expect(screen.getByText("2. Step 2 prompt body.")).toBeTruthy();
    expect(screen.queryByText("3. Step 3 prompt body.")).toBeNull();
  });

  it("renders hover preview as a floating tooltip outside the listbox", () => {
    vi.useFakeTimers();
    renderQuickList();

    const listbox = screen.getByRole("listbox", { name: "提示词" });
    revealHoverPreview(screen.getByRole("option", { name: /修复流程/i }));

    const tooltip = screen.getByRole("tooltip");
    expect(listbox.contains(tooltip)).toBe(false);
    expect(tooltip.className).toContain("prompt-hover-preview");
    expect(tooltip.className).toContain("prompt-hover-preview-floating");
  });

  it("anchors hover preview to the hovered prompt container", () => {
    vi.useFakeTimers();
    renderQuickList();

    const shell = document.querySelector(".prompt-quick-shell") as HTMLElement;
    vi.spyOn(shell, "getBoundingClientRect").mockReturnValue(
      makeRect(20, 30, 360, 340)
    );
    Object.defineProperty(shell, "clientWidth", { configurable: true, value: 360 });
    Object.defineProperty(shell, "clientHeight", { configurable: true, value: 340 });
    const option = screen.getByRole("option", { name: /修复流程/i });
    vi.spyOn(option, "getBoundingClientRect").mockReturnValue(
      makeRect(40, 180, 300, 90)
    );

    fireEvent.pointerMove(option, { clientX: 90, clientY: 330 });
    act(() => {
      vi.advanceTimersByTime(1500);
    });

    const tooltip = screen.getByRole("tooltip");
    expect(tooltip.className).toContain("is-above");
    expect(tooltip.style.left).toBe("20px");
    expect(tooltip.style.top).toBe("142px");
    expect(tooltip.style.width).toBe("300px");
  });

  it("delays hover preview for 1.5 seconds", () => {
    vi.useFakeTimers();
    renderQuickList();

    fireEvent.pointerMove(screen.getByRole("option", { name: /讨论方案/i }));

    act(() => {
      vi.advanceTimersByTime(1499);
    });
    expect(screen.queryByRole("tooltip")).toBeNull();

    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.getByRole("tooltip")).toBeTruthy();
  });

  it("does not start hover preview from pointer enter alone", () => {
    vi.useFakeTimers();
    renderQuickList();

    const option = screen.getByRole("option", { name: /讨论方案/i });
    fireEvent.pointerEnter(option);

    act(() => {
      vi.advanceTimersByTime(2000);
    });

    expect(screen.queryByRole("tooltip")).toBeNull();

    fireEvent.pointerMove(option);
    act(() => {
      vi.advanceTimersByTime(1500);
    });

    expect(screen.getByRole("tooltip")).toBeTruthy();
  });

  it("reports group preview on pointer enter without starting the hover tooltip", () => {
    vi.useFakeTimers();
    const onGroupPreview = vi.fn();
    renderQuickList({ onGroupPreview });

    fireEvent.pointerEnter(screen.getByRole("option", { name: /修复流程/i }));
    act(() => {
      vi.advanceTimersByTime(2000);
    });

    expect(onGroupPreview).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("does not report group preview for single prompts", () => {
    const onGroupPreview = vi.fn();
    renderQuickList({ onGroupPreview });

    fireEvent.pointerEnter(screen.getByRole("option", { name: /讨论方案/i }));

    expect(onGroupPreview).not.toHaveBeenCalled();
  });

  it("clears hover preview when the reset key changes", () => {
    vi.useFakeTimers();
    const { rerender } = renderQuickList({ hoverResetKey: 0 });

    revealHoverPreview(screen.getByRole("option", { name: /修复流程/i }));
    expect(screen.getByRole("tooltip")).toBeTruthy();

    const zh = getMessages("zh-CN");
    rerender(
      <PromptQuickList
        prompts={prompts}
        messages={zh.quickList}
        groupMeta={zh.manager.groupMeta}
        onSelect={() => {}}
        hoverResetKey={1}
      />
    );

    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("cancels a pending hover preview when the reset key changes", () => {
    vi.useFakeTimers();
    const { rerender } = renderQuickList({ hoverResetKey: 0 });

    fireEvent.pointerMove(screen.getByRole("option", { name: /修复流程/i }));

    const zh = getMessages("zh-CN");
    rerender(
      <PromptQuickList
        prompts={prompts}
        messages={zh.quickList}
        groupMeta={zh.manager.groupMeta}
        onSelect={() => {}}
        hoverResetKey={1}
      />
    );

    act(() => {
      vi.advanceTimersByTime(1500);
    });

    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("does not show hover preview from focus alone", () => {
    vi.useFakeTimers();
    renderQuickList();

    fireEvent.focus(screen.getByRole("option", { name: /修复流程/i }));
    act(() => {
      vi.advanceTimersByTime(1500);
    });

    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("shows full single prompt content on hover", () => {
    vi.useFakeTimers();
    renderQuickList();

    revealHoverPreview(screen.getByRole("option", { name: /讨论方案/i }));

    const tooltip = screen.getByRole("tooltip");
    expect(tooltip.textContent).toContain(
      "使用 brainstorming skill，先和我讨论方案，不要修改代码。"
    );
    expect(tooltip.querySelector(".prompt-hover-preview-header")).toBeNull();
    expect(tooltip.querySelector("strong")).toBeNull();
  });

  it("shows full group prompt content on hover", () => {
    vi.useFakeTimers();
    renderQuickList();

    expect(screen.queryByRole("tooltip")).toBeNull();

    const option = screen.getByRole("option", { name: /修复流程/i });
    revealHoverPreview(option);

    const tooltip = screen.getByRole("tooltip");
    expect(tooltip.textContent).toContain("1. 分析根本原因。");
    expect(tooltip.textContent).toContain("2. 执行修复。");
    expect(tooltip.textContent).toContain("3. 完成验证。");
    expect(tooltip.textContent).not.toContain("修复流程");
    expect(tooltip.textContent).not.toContain("群组 · 3 条");

    fireEvent.pointerLeave(option);

    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("does not render management actions", () => {
    renderQuickList();

    expect(screen.queryByText("管理提示词")).toBeNull();
    expect(screen.queryByText("添加提示词")).toBeNull();
    expect(screen.queryByText("导入")).toBeNull();
    expect(screen.queryByText("导出")).toBeNull();
  });

  it("selects the whole container on click", () => {
    let selected: PromptContainer | null = null;
    renderQuickList({ onSelect: (prompt) => { selected = prompt; } });

    fireEvent.click(screen.getByText("修复流程"));

    expect(selected).toEqual(prompts[1]);
  });

  it("keeps prompt options selectable after hover preview appears", () => {
    vi.useFakeTimers();
    let selected: PromptContainer | null = null;
    renderQuickList({ onSelect: (prompt) => { selected = prompt; } });

    const option = screen.getByRole("option", { name: /修复流程/i });
    revealHoverPreview(option);
    fireEvent.click(option);

    expect(selected).toEqual(prompts[1]);
  });

  it("clears hover preview before selecting a prompt", () => {
    vi.useFakeTimers();
    let selected: PromptContainer | null = null;
    renderQuickList({ onSelect: (prompt) => { selected = prompt; } });

    const option = screen.getByRole("option", { name: /修复流程/i });
    revealHoverPreview(option);
    fireEvent.click(option);

    expect(selected).toEqual(prompts[1]);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("disables the prompt currently being submitted", () => {
    renderQuickList({ submittingPromptId: "1" });

    expect(
      (screen.getByRole("option", { name: /讨论方案/i }) as HTMLButtonElement)
        .disabled
    ).toBe(true);
  });
});
