import { describe, expect, it } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import type { PromptItem } from "../shared/promptTypes";
import { PromptQuickList } from "./PromptQuickList";

const prompts: PromptItem[] = [
  {
    id: "1",
    title: "讨论方案",
    body: "使用 brainstorming skill，先和我讨论方案，不要修改代码。",
    order: 0,
    createdAt: "2026-05-26T00:00:00.000Z",
    updatedAt: "2026-05-26T00:00:00.000Z"
  }
];

describe("PromptQuickList", () => {
  it("renders compact prompt options", () => {
    render(<PromptQuickList prompts={prompts} onSelect={() => {}} />);

    expect(screen.getByText("讨论方案")).toBeTruthy();
    expect(screen.getByText(/brainstorming skill/)).toBeTruthy();
  });

  it("does not render management actions", () => {
    render(<PromptQuickList prompts={prompts} onSelect={() => {}} />);

    expect(screen.queryByText("Manage Prompts")).toBeNull();
    expect(screen.queryByText("Add Prompt")).toBeNull();
    expect(screen.queryByText("Import")).toBeNull();
    expect(screen.queryByText("Export")).toBeNull();
  });

  it("selects prompt on click", () => {
    let selected: PromptItem | null = null;
    render(<PromptQuickList prompts={prompts} onSelect={(prompt) => { selected = prompt; }} />);

    fireEvent.click(screen.getByText("讨论方案"));

    expect(selected).toEqual(prompts[0]);
  });
});