// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { TaskStatus } from "../lib/types";
import { TaskSections } from "./TaskSections";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root | undefined;

afterEach(() => {
  act(() => root?.unmount());
  root = undefined;
  document.body.innerHTML = "";
});

describe("今天页任务分组", () => {
  it("把已完成任务放入默认折叠的已完成区，待办仍显示在前面", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);

    await act(async () =>
      root!.render(
        <TaskSections
          tasks={[
            { id: "done-1", title: "已经完成", status: "done" as TaskStatus },
            { id: "todo-1", title: "还没完成", status: "todo" as TaskStatus },
          ]}
          separateCompleted
          renderTask={(task) => <li data-task-id={task.id}>{task.title}</li>}
        />,
      ),
    );

    const pendingList = document.querySelector('ul[aria-label="待处理任务"]');
    const completedSection = document.querySelector(
      "details.tasklist-completed",
    ) as HTMLDetailsElement | null;
    expect(pendingList?.textContent).toContain("还没完成");
    expect(pendingList?.textContent).not.toContain("已经完成");
    expect(completedSection?.open).toBe(false);
    expect(completedSection?.querySelector("summary")?.textContent).toContain(
      "已完成（1 项）",
    );
    expect(completedSection?.querySelector("ul")?.textContent).toContain(
      "已经完成",
    );
    const sectionText =
      host.querySelector(".tasklist-sections")?.textContent ?? "";
    expect(sectionText.indexOf("还没完成")).toBeLessThan(
      sectionText.indexOf("已完成"),
    );

    await act(async () =>
      completedSection
        ?.querySelector("summary")
        ?.dispatchEvent(new MouseEvent("click", { bubbles: true })),
    );
    expect(completedSection?.open).toBe(true);
  });
});
