import { describe, expect, it, vi } from "vitest";
import { act, render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { PromptManager } from "./PromptManager";
import { getMessages } from "../shared/i18n";
import type { PromptCategory, PromptContainer } from "../shared/promptTypes";

describe("prompt manager", () => {
  const mockPrompts: PromptContainer[] = [
    {
      id: "1",
      categoryId: "category-default",
      title: "Code Review",
      type: "single",
      sendBehavior: "inherit",
      prompts: [{ id: "1-entry", body: "Review this code for bugs.", order: 0 }],
      intervalMs: 700,
      order: 0,
      createdAt: "2026-05-26T00:00:00.000Z",
      updatedAt: "2026-05-26T00:00:00.000Z"
    },
    {
      id: "2",
      categoryId: "category-default",
      title: "Repair Group",
      type: "group",
      sendBehavior: "inherit",
      prompts: [
        { id: "2-entry-1", body: "Analyze root cause.", order: 0 },
        { id: "2-entry-2", body: "Execute the fix.", order: 1 },
      ],
      intervalMs: 700,
      order: 1,
      createdAt: "2026-05-26T00:00:00.000Z",
      updatedAt: "2026-05-26T00:00:00.000Z"
    }
  ];

  const defaultCategory: PromptCategory = {
    id: "category-default",
    name: "Default",
    order: 0,
    createdAt: "2026-05-26T00:00:00.000Z",
    updatedAt: "2026-05-26T00:00:00.000Z",
  };

  function renderManager(overrides: Partial<Parameters<typeof PromptManager>[0]> = {}) {
    const props = {
      prompts: mockPrompts,
      dividers: [],
      categories: [defaultCategory],
      activeCategoryId: defaultCategory.id,
      categoryCounts: { [defaultCategory.id]: mockPrompts.length },
      totalPromptCount: mockPrompts.length,
      onCreate: () => {},
      onCreateGroup: () => {},
      onCombineSingles: () => {},
      onSplitGroup: () => {},
      onUpdate: () => {},
      onDelete: () => {},
      onReorder: () => {},
      onCreateDivider: () => {},
      onUpdateDivider: () => {},
      onDeleteDivider: () => {},
      onSelectCategory: () => {},
      onCreateCategory: () => {},
      onRenameCategory: () => {},
      onDeleteCategory: () => {},
      getCategoryDisplayName: (category: PromptCategory) =>
        category.id === "category-default" && category.name === "Default" ? "默认" : category.name,
      onImport: () => {},
      onExport: () => {},
      messages: getMessages("zh-CN"),
      onOpenSettings: () => {},
      ...overrides,
    };
    const result = render(<PromptManager {...props} />);
    return { props, ...result };
  }

  function makePrompt(overrides: Partial<PromptContainer>): PromptContainer {
    return {
      id: "prompt",
      categoryId: "category-default",
      title: "Prompt",
      type: "single",
      sendBehavior: "inherit",
      prompts: [{ id: "entry", body: "Body", order: 0 }],
      intervalMs: 700,
      order: 0,
      createdAt: "2026-05-26T00:00:00.000Z",
      updatedAt: "2026-05-26T00:00:00.000Z",
      ...overrides,
    };
  }

  function openCreatePanel(name = "+ 添加提示词") {
    fireEvent.click(screen.getByRole("button", { name }));
  }

  it("renders prompt containers with group distinction", () => {
    renderManager();

    expect(screen.getByText("Code Review")).toBeTruthy();
    expect(screen.getByText("Repair Group")).toBeTruthy();
    expect(screen.queryByText("Single · 1 prompt")).toBeNull();
    expect(screen.getByText("群组 · 2 条")).toBeTruthy();
    expect(screen.queryByText(/700ms/)).toBeNull();
  });

  it("does not render instructional manager section descriptions", () => {
    renderManager();

    expect(screen.queryByText("为快速选择器添加一个提示词或一个有顺序的提示词组。")).toBeNull();
    expect(screen.queryByText("选择小猫列表中的显示顺序。")).toBeNull();
  });

  it("keeps the create panel collapsed until adding a prompt", () => {
    renderManager();

    expect(screen.queryByRole("heading", { name: "新建提示词容器" })).toBeNull();

    openCreatePanel();

    const singleButton = screen.getByRole("button", { name: "单个" });
    const header = singleButton.closest(".panel-heading-with-actions");

    expect(header).toBeTruthy();
    expect(header?.textContent).toContain("新建提示词容器");
  });

  it("renders the create action in a right-aligned form action row", () => {
    renderManager();

    openCreatePanel();

    const addButton = screen.getByRole("button", { name: "添加提示词" });
    const actionRow = addButton.closest(".editor-submit-row");

    expect(actionRow).toBeTruthy();
    expect(actionRow?.textContent).toContain("添加提示词");
  });

  it("marks prompt container type segments with pressed state", () => {
    renderManager();

    openCreatePanel();

    expect(screen.getByRole("button", { name: "单个" }).getAttribute("aria-pressed"))
      .toBe("true");
    expect(screen.getByRole("button", { name: "群组" }).getAttribute("aria-pressed"))
      .toBe("false");

    fireEvent.click(screen.getByRole("button", { name: "群组" }));

    expect(screen.getByRole("button", { name: "单个" }).getAttribute("aria-pressed"))
      .toBe("false");
    expect(screen.getByRole("button", { name: "群组" }).getAttribute("aria-pressed"))
      .toBe("true");
  });

  it("does not render a container-level send behavior control", () => {
    renderManager();

    openCreatePanel();

    expect(screen.queryByText("发送行为")).toBeNull();
    expect(screen.queryByRole("button", { name: "继承设置" })).toBeNull();
    expect(screen.queryByRole("button", { name: "填入 + Cmd Enter" })).toBeNull();
  });

  it("renders prompt list as a unified row list", () => {
    renderManager();

    const list = screen.getByText("Code Review").closest(".prompt-list");

    expect(list).toBeTruthy();
    expect(list?.querySelectorAll(".prompt-item").length).toBe(2);
  });

  it("enables Motion reordering for every prompt container", () => {
    renderManager();

    const list = document.querySelector('[data-reorder-list="true"]');
    const rows = Array.from(document.querySelectorAll<HTMLElement>("[data-reorder-id]"));

    expect(list).toBeTruthy();
    expect(rows.map((row) => row.dataset.reorderId)).toEqual(["1", "2"]);
    expect(rows.every((row) => row.classList.contains("is-reorder-enabled"))).toBe(true);
  });

  it("disables row dragging while a prompt is being edited", () => {
    renderManager();

    fireEvent.click(screen.getAllByRole("button", { name: "编辑" })[0]);

    const rows = Array.from(document.querySelectorAll<HTMLElement>("[data-reorder-id]"));
    expect(rows.every((row) => !row.classList.contains("is-reorder-enabled"))).toBe(true);
  });

  it("renders a left category rail with the list-first manager layout", () => {
    renderManager({
      categories: [
        { id: "cat-dev", name: "开发代码", order: 0, createdAt: "", updatedAt: "" },
        { id: "cat-writing", name: "写作", order: 1, createdAt: "", updatedAt: "" },
      ],
      activeCategoryId: "cat-dev",
      categoryCounts: { "cat-dev": 2, "cat-writing": 0 },
    });

    expect(screen.getByRole("heading", { name: "分类" })).toBeTruthy();
    expect(screen.getByRole("button", { name: /开发代码.*2/ })).toBeTruthy();
    expect(screen.getByRole("heading", { name: "提示词列表" })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "新建提示词容器" })).toBeNull();
    openCreatePanel();
    expect(screen.getByRole("heading", { name: "新建提示词容器" })).toBeTruthy();
  });

  it("clears transient edit and delete state when the active category changes", () => {
    const categories = [
      { id: "cat-dev", name: "开发代码", order: 0, createdAt: "", updatedAt: "" },
      { id: "cat-writing", name: "写作", order: 1, createdAt: "", updatedAt: "" },
    ];
    const { rerender, props } = renderManager({
      categories,
      activeCategoryId: "cat-dev",
      categoryCounts: { "cat-dev": 1, "cat-writing": 1 },
      prompts: [makePrompt({ id: "dev-1", categoryId: "cat-dev", title: "Code Review" })],
    });

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));
    expect(screen.getByDisplayValue("Code Review")).toBeTruthy();

    rerender(
      <PromptManager
        {...props}
        activeCategoryId="cat-writing"
        prompts={[makePrompt({ id: "writing-1", categoryId: "cat-writing", title: "Blog Draft" })]}
      />
    );

    expect(screen.queryByDisplayValue("Code Review")).toBeNull();
  });

  it("does not render a legacy send behavior override while editing", () => {
    const updated: Array<Record<string, unknown>> = [];
    renderManager({
      prompts: [
        makePrompt({
          id: "command-prompt",
          title: "Command Prompt",
          sendBehavior: "paste_command_enter",
        }),
      ],
      onUpdate: (_id, input) => { updated.push(input); },
    });

    fireEvent.click(screen.getByRole("button", { name: "编辑" }));

    expect(screen.queryByText("发送行为")).toBeNull();
    expect(screen.queryByRole("button", { name: "继承设置" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "保存" }));

    expect(updated[0]?.sendBehavior).toBeUndefined();
  });

  it("creates a single prompt container", async () => {
    let created: { title: string; body: string } | null = null;
    renderManager({ onCreate: (input) => { created = input; } });

    openCreatePanel();
    fireEvent.change(screen.getByPlaceholderText("标题"), {
      target: { value: "New Single" },
    });
    fireEvent.change(screen.getByPlaceholderText("提示词内容..."), {
      target: { value: "Single body" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加提示词" }));

    expect(created).toEqual({
      title: "New Single",
      body: "Single body",
    });
    await waitFor(() => {
      expect(screen.queryByRole("heading", { name: "新建提示词容器" })).toBeNull();
    });
  });

  it("cancels the inline create form without keeping the draft", () => {
    renderManager();

    openCreatePanel();
    fireEvent.change(screen.getByPlaceholderText("标题"), {
      target: { value: "Discard me" },
    });
    fireEvent.click(screen.getByRole("button", { name: "取消" }));

    expect(screen.queryByRole("heading", { name: "新建提示词容器" })).toBeNull();

    openCreatePanel();

    expect((screen.getByPlaceholderText("标题") as HTMLInputElement).value).toBe("");
  });

  it("shows localized success feedback after creating a single prompt", async () => {
    renderManager({ messages: getMessages("en-US") });

    openCreatePanel("+ Add Prompt");
    fireEvent.change(screen.getByPlaceholderText("Title"), {
      target: { value: "New Single" },
    });
    fireEvent.change(screen.getByPlaceholderText("Prompt body..."), {
      target: { value: "Single body" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add Prompt" }));

    expect(await screen.findByText("Prompt added")).toBeTruthy();
  });

  it("creates a single prompt on the first click while the body field is focused", () => {
    let created: { title: string; body: string } | null = null;
    renderManager({ onCreate: (input) => { created = input; } });

    openCreatePanel();
    fireEvent.change(screen.getByPlaceholderText("标题"), {
      target: { value: "审阅修复计划" },
    });
    const bodyField = screen.getByPlaceholderText("提示词内容...");
    fireEvent.change(bodyField, {
      target: { value: "你深入分析一下..." },
    });
    bodyField.focus();
    fireEvent.pointerDown(screen.getByRole("button", { name: "添加提示词" }));
    fireEvent.pointerUp(screen.getByRole("button", { name: "添加提示词" }));
    fireEvent.click(screen.getByRole("button", { name: "添加提示词" }));

    expect(created).toEqual({
      title: "审阅修复计划",
      body: "你深入分析一下...",
    });
  });

  it("does not create duplicate prompts from pointer and click events in the same gesture", () => {
    let createCount = 0;
    renderManager({ onCreate: () => { createCount += 1; } });

    openCreatePanel();
    fireEvent.change(screen.getByPlaceholderText("标题"), {
      target: { value: "No duplicate" },
    });
    fireEvent.change(screen.getByPlaceholderText("提示词内容..."), {
      target: { value: "Only once" },
    });
    const addButton = screen.getByRole("button", { name: "添加提示词" });
    fireEvent.pointerDown(addButton);
    fireEvent.pointerUp(addButton);
    fireEvent.click(addButton);

    expect(createCount).toBe(1);
  });

  it("creates a group with numbered prompts", () => {
    let createdGroup: {
      title: string;
      prompts: Array<{ body: string }>;
      intervalMs: number;
    } | null = null;
    renderManager({ onCreateGroup: (input) => { createdGroup = input; } });

    openCreatePanel();
    fireEvent.click(screen.getByRole("button", { name: "群组" }));
    fireEvent.change(screen.getByPlaceholderText("标题"), {
      target: { value: "Codex Flow" },
    });
    const promptFields = screen.getAllByRole("textbox").filter((field) => {
      return field.getAttribute("placeholder") !== "标题";
    });
    fireEvent.change(promptFields[0], { target: { value: "First grouped prompt" } });
    fireEvent.change(promptFields[1], { target: { value: "Second grouped prompt" } });

    expect(screen.getByText("提示词 1")).toBeTruthy();
    expect(screen.queryByText(/Step/i)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "添加群组" }));

    expect(createdGroup).toEqual({
      title: "Codex Flow",
      prompts: [
        { body: "First grouped prompt", order: 0 },
        { body: "Second grouped prompt", order: 1 },
      ],
      intervalMs: 700,
    });
  });

  it("shows group interval in seconds while creating a group", () => {
    renderManager();

    openCreatePanel();
    fireEvent.click(screen.getByRole("button", { name: "群组" }));

    const intervalInput = screen.getByLabelText("提示词间隔") as HTMLInputElement;
    expect(intervalInput.value).toBe("0.7");
    expect(screen.getByText("s")).toBeTruthy();
    expect(screen.queryByText("ms")).toBeNull();
  });

  it("converts group interval seconds to milliseconds when creating a group", () => {
    let createdIntervalMs: number | null = null;
    renderManager({ onCreateGroup: (input) => { createdIntervalMs = input.intervalMs; } });

    openCreatePanel();
    fireEvent.click(screen.getByRole("button", { name: "群组" }));
    fireEvent.change(screen.getByPlaceholderText("标题"), {
      target: { value: "Timed Group" },
    });
    fireEvent.change(screen.getAllByLabelText(/提示词 \d+ 内容/i)[0], {
      target: { value: "First" },
    });
    fireEvent.change(screen.getByLabelText("提示词间隔"), {
      target: { value: "1.5" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加群组" }));

    expect(createdIntervalMs).toBe(1500);
  });

  it("shows existing group interval in seconds while editing", () => {
    renderManager();

    fireEvent.click(screen.getAllByRole("button", { name: "编辑" })[1]);

    expect((screen.getByLabelText("提示词间隔") as HTMLInputElement).value).toBe("0.7");
  });

  it("shows success feedback after creating a group", async () => {
    renderManager();

    openCreatePanel();
    fireEvent.click(screen.getByRole("button", { name: "群组" }));
    fireEvent.change(screen.getByPlaceholderText("标题"), {
      target: { value: "群组提示词" },
    });
    fireEvent.change(screen.getAllByLabelText(/提示词 \d+ 内容/i)[0], {
      target: { value: "第一条" },
    });
    fireEvent.click(screen.getByRole("button", { name: "添加群组" }));

    expect(await screen.findByText("已添加提示词组")).toBeTruthy();
  });

  it("inserts and removes group prompts from row controls", () => {
    renderManager();

    openCreatePanel();
    fireEvent.click(screen.getByRole("button", { name: "群组" }));
    expect(screen.getAllByLabelText(/提示词 \d+ 内容/i)).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: "在提示词 1 后插入" }));
    expect(screen.getAllByLabelText(/提示词 \d+ 内容/i)).toHaveLength(3);

    fireEvent.click(screen.getByRole("button", { name: "移除提示词 2" }));
    expect(screen.getAllByLabelText(/提示词 \d+ 内容/i)).toHaveLength(2);
  });

  it("reorders group prompts from row controls", () => {
    renderManager();

    openCreatePanel();
    fireEvent.click(screen.getByRole("button", { name: "群组" }));
    const promptFields = screen.getAllByLabelText(/提示词 \d+ 内容/i);
    fireEvent.change(promptFields[0], { target: { value: "First grouped prompt" } });
    fireEvent.change(promptFields[1], { target: { value: "Second grouped prompt" } });

    fireEvent.click(screen.getByRole("button", { name: "上移提示词 2" }));

    const reorderedFields = screen.getAllByLabelText(/提示词 \d+ 内容/i);
    expect((reorderedFields[0] as HTMLTextAreaElement).value).toBe("Second grouped prompt");
    expect((reorderedFields[1] as HTMLTextAreaElement).value).toBe("First grouped prompt");
  });

  it("does not remove the last group prompt row", () => {
    renderManager();

    openCreatePanel();
    fireEvent.click(screen.getByRole("button", { name: "群组" }));
    fireEvent.click(screen.getByRole("button", { name: "移除提示词 2" }));

    expect((screen.getByRole("button", { name: "移除提示词 1" }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect(screen.getAllByLabelText(/提示词 \d+ 内容/i)).toHaveLength(1);
  });

  it("reorders group prompts by dragging the row handle", () => {
    renderManager();

    openCreatePanel();
    fireEvent.click(screen.getByRole("button", { name: "群组" }));
    const promptFields = screen.getAllByLabelText(/提示词 \d+ 内容/i);
    fireEvent.change(promptFields[0], { target: { value: "First grouped prompt" } });
    fireEvent.change(promptFields[1], { target: { value: "Second grouped prompt" } });

    const dataTransfer = {
      effectAllowed: "",
      dropEffect: "",
      setData: () => {},
      getData: () => "",
    };

    fireEvent.dragStart(screen.getByRole("button", { name: "拖拽提示词 1" }), {
      dataTransfer,
    });
    fireEvent.dragOver(screen.getByLabelText("提示词 2 内容"), { dataTransfer });
    fireEvent.drop(screen.getByLabelText("提示词 2 内容"), { dataTransfer });

    const reorderedFields = screen.getAllByLabelText(/提示词 \d+ 内容/i);
    expect((reorderedFields[0] as HTMLTextAreaElement).value).toBe("Second grouped prompt");
    expect((reorderedFields[1] as HTMLTextAreaElement).value).toBe("First grouped prompt");
  });

  it("asks for confirmation before delete", () => {
    renderManager();

    fireEvent.click(screen.getByRole("button", { name: "Code Review 的更多操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "删除" }));

    expect(screen.getByText("删除这个提示词？")).toBeTruthy();
  });

  it("deletes after confirmation", () => {
    let deleteId: string | null = null;
    renderManager({ onDelete: (id: string) => { deleteId = id; } });

    fireEvent.click(screen.getByRole("button", { name: "Code Review 的更多操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "删除" }));
    fireEvent.click(screen.getByText("确认"));

    expect(deleteId).toBe("1");
  });

  it("uses the same standard actions and overflow menu for singles and groups", () => {
    renderManager();

    expect(screen.getAllByRole("button", { name: "编辑" })).toHaveLength(2);
    expect(screen.getByRole("button", { name: "上移 Code Review" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "下移 Code Review" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "上移 Repair Group" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "下移 Repair Group" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Code Review 的更多操作" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Repair Group 的更多操作" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Code Review 的更多操作" }));
    expect(screen.getByRole("menuitem", { name: "删除" })).toBeTruthy();
    expect(screen.queryByRole("menuitem", { name: "拆分为单条" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Repair Group 的更多操作" }));
    expect(screen.getByRole("menuitem", { name: "拆分为单条" })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: "删除" })).toBeTruthy();
  });

  it("combines non-adjacent single prompts with source deletion enabled by default", async () => {
    const combineCalls: Array<{
      ids: string[];
      title: string;
      deleteOriginals: boolean;
    }> = [];
    const singles = [
      makePrompt({ id: "1", title: "First", order: 0 }),
      makePrompt({ id: "2", title: "Second", order: 1 }),
      makePrompt({ id: "3", title: "Third", order: 2 }),
    ];
    renderManager({
      prompts: singles,
      categoryCounts: { [defaultCategory.id]: singles.length },
      totalPromptCount: singles.length,
      onCombineSingles: (input) => { combineCalls.push(input); },
    });

    fireEvent.click(screen.getByRole("button", { name: "选择" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 First" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Third" }));
    fireEvent.click(screen.getByRole("button", { name: "合并为群组" }));

    await waitFor(() => {
      expect((screen.getByDisplayValue("新提示词组") as HTMLInputElement).selectionStart).toBe(0);
    });
    expect((screen.getByRole("checkbox", {
      name: /合并后删除原来的 2 个单条提示词/,
    }) as HTMLInputElement).checked).toBe(true);
    fireEvent.change(screen.getByDisplayValue("新提示词组"), {
      target: { value: "Combined" },
    });
    fireEvent.click(screen.getByRole("button", { name: "创建群组并删除原提示词" }));

    await waitFor(() => expect(combineCalls).toEqual([{
      ids: ["1", "3"],
      title: "Combined",
      deleteOriginals: true,
    }]));
  });

  it("reorders selected prompts and keeps source singles when requested", async () => {
    const combineCalls: Array<{
      ids: string[];
      title: string;
      deleteOriginals: boolean;
    }> = [];
    const singles = [
      makePrompt({ id: "1", title: "First", order: 0 }),
      makePrompt({ id: "2", title: "Second", order: 1 }),
      makePrompt({ id: "3", title: "Third", order: 2 }),
    ];
    renderManager({
      prompts: singles,
      categoryCounts: { [defaultCategory.id]: singles.length },
      totalPromptCount: singles.length,
      onCombineSingles: (input) => { combineCalls.push(input); },
    });

    fireEvent.click(screen.getByRole("button", { name: "选择" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 First" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Third" }));
    fireEvent.click(screen.getByRole("button", { name: "合并为群组" }));
    expect((screen.getByRole("button", { name: "上移 First" }) as HTMLButtonElement).disabled)
      .toBe(true);
    expect((screen.getByRole("button", { name: "下移 Third" }) as HTMLButtonElement).disabled)
      .toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "上移 Third" }));
    fireEvent.click(screen.getByRole("checkbox", {
      name: /合并后删除原来的 2 个单条提示词/,
    }));
    fireEvent.change(screen.getByDisplayValue("新提示词组"), {
      target: { value: "Combined" },
    });
    fireEvent.click(screen.getByRole("button", { name: "创建群组" }));

    await waitFor(() => expect(combineCalls).toEqual([{
      ids: ["3", "1"],
      title: "Combined",
      deleteOriginals: false,
    }]));
  });

  it("keeps combine single-flight until persistence settles", async () => {
    let resolveCombine: (() => void) | null = null;
    const pendingCombine = new Promise<void>((resolve) => {
      resolveCombine = resolve;
    });
    let combineCount = 0;
    const singles = [
      makePrompt({ id: "1", title: "First", order: 0 }),
      makePrompt({ id: "2", title: "Second", order: 1 }),
    ];
    renderManager({
      prompts: singles,
      categoryCounts: { [defaultCategory.id]: singles.length },
      totalPromptCount: singles.length,
      onCombineSingles: async () => {
        combineCount += 1;
        await pendingCombine;
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "选择" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 First" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Second" }));
    fireEvent.click(screen.getByRole("button", { name: "合并为群组" }));
    const submit = screen.getByRole("button", { name: "创建群组并删除原提示词" });
    fireEvent.click(submit);
    fireEvent.submit(submit.closest("form")!);

    expect(combineCount).toBe(1);
    expect((submit as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByDisplayValue("新提示词组") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByRole("checkbox", {
      name: /合并后删除原来的 2 个单条提示词/,
    }) as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByRole("button", { name: "下移 First" }) as HTMLButtonElement).disabled)
      .toBe(true);
    const removeFirst = screen.getByRole("button", { name: "从合并中移除 First" });
    expect((removeFirst as HTMLButtonElement).disabled).toBe(true);
    const mergeDialog = screen.getByRole("dialog", { name: "合并为提示词组" });
    expect((within(mergeDialog).getByRole("button", { name: "取消" }) as HTMLButtonElement).disabled)
      .toBe(true);
    fireEvent.click(removeFirst);
    expect(mergeDialog).toBeTruthy();
    expect(screen.getAllByText("First").length).toBeGreaterThan(0);

    await act(async () => {
      resolveCombine?.();
      await pendingCombine;
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "合并为提示词组" })).toBeNull();
    });
  });

  it("keeps the combine dialog open and reports persistence failures", async () => {
    const singles = [
      makePrompt({ id: "1", title: "First", order: 0 }),
      makePrompt({ id: "2", title: "Second", order: 1 }),
    ];
    renderManager({
      prompts: singles,
      categoryCounts: { [defaultCategory.id]: singles.length },
      totalPromptCount: singles.length,
      onCombineSingles: async () => { throw new Error("write failed"); },
    });

    fireEvent.click(screen.getByRole("button", { name: "选择" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 First" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "选择 Second" }));
    fireEvent.click(screen.getByRole("button", { name: "合并为群组" }));
    fireEvent.click(screen.getByRole("button", { name: "创建群组并删除原提示词" }));

    expect((await screen.findByRole("alert")).textContent).toContain("合并失败");
    expect(screen.getByRole("dialog", { name: "合并为提示词组" })).toBeTruthy();
  });

  it("splits a group from the same row menu used by single prompts", async () => {
    const splitIds: string[] = [];
    renderManager({ onSplitGroup: (id) => { splitIds.push(id); } });

    fireEvent.click(screen.getByRole("button", { name: "Repair Group 的更多操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "拆分为单条" }));

    expect(screen.getByRole("dialog", { name: "拆分提示词组" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "确认拆分" }));

    await waitFor(() => expect(splitIds).toEqual(["2"]));
  });

  it("keeps the split dialog open and reports persistence failures", async () => {
    renderManager({
      onSplitGroup: async () => { throw new Error("write failed"); },
    });

    fireEvent.click(screen.getByRole("button", { name: "Repair Group 的更多操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "拆分为单条" }));
    fireEvent.click(screen.getByRole("button", { name: "确认拆分" }));

    expect((await screen.findByRole("alert")).textContent).toContain("拆分失败");
    expect(screen.getByRole("dialog", { name: "拆分提示词组" })).toBeTruthy();
  });

  it("freezes split confirmation and cancellation until persistence settles", async () => {
    let resolveSplit: (() => void) | null = null;
    const pendingSplit = new Promise<void>((resolve) => {
      resolveSplit = resolve;
    });
    renderManager({ onSplitGroup: async () => pendingSplit });

    fireEvent.click(screen.getByRole("button", { name: "Repair Group 的更多操作" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "拆分为单条" }));
    fireEvent.click(screen.getByRole("button", { name: "确认拆分" }));

    expect((screen.getByRole("button", { name: "确认拆分" }) as HTMLButtonElement).disabled)
      .toBe(true);
    const splitDialog = screen.getByRole("dialog", { name: "拆分提示词组" });
    expect((within(splitDialog).getByRole("button", { name: "取消" }) as HTMLButtonElement).disabled)
      .toBe(true);

    await act(async () => {
      resolveSplit?.();
      await pendingSplit;
    });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "拆分提示词组" })).toBeNull();
    });
  });

  it("calls reorder with new order when moving down", () => {
    let reorderIds: string[] | null = null;
    renderManager({ onReorder: (ids: string[]) => { reorderIds = ids; } });

    const moveDownBtn = screen.getAllByText("↓")[0];
    fireEvent.click(moveDownBtn);

    expect(reorderIds).toEqual(["2", "1"]);
  });

  it("restores the previous order and reports reorder persistence failures", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      renderManager({
        onReorder: async () => {
          throw new Error("write failed");
        },
      });

      fireEvent.click(screen.getByRole("button", { name: "下移 Code Review" }));

      expect((await screen.findByRole("alert")).textContent).toContain("排序保存失败");
      await waitFor(() => {
        const rows = screen.getAllByRole("listitem");
        expect(rows[0].textContent).toContain("Code Review");
        expect(rows[1].textContent).toContain("Repair Group");
      });
    } finally {
      consoleError.mockRestore();
    }
  });

  it("exposes import and export actions", () => {
    renderManager();

    expect(screen.getByText("设置")).toBeTruthy();
    expect(screen.getByText("导入")).toBeTruthy();
    expect(screen.getByText("导出")).toBeTruthy();
  });

  it("opens settings from the manager header", () => {
    let opened = false;
    renderManager({ onOpenSettings: () => { opened = true; } });

    fireEvent.click(screen.getByRole("button", { name: "设置" }));

    expect(opened).toBe(true);
  });
});
