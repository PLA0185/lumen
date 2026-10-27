import { describe, it, expect } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { FloatingTaskSections } from "./FloatingTaskSections";
import { FloatingToday } from "./FloatingToday";
import { QuickAdd } from "./QuickAdd";
import type { Task } from "../lib/types";

describe("浮窗直接新建与一周导航", () => {
  it("直接提供日期导航、新建、专注和提醒入口", () => {
    const html = renderToStaticMarkup(<FloatingToday />);
    expect(html).toContain('aria-label="一周日期"');
    expect(html).toContain('aria-label="新建任务"');
    expect(html).toContain("开始专注");
    expect(html).toContain("添加提醒");
  });
  it("新建行提供就地编辑选项，默认沿用选中日期", () => {
    const html = renderToStaticMarkup(
      <QuickAdd compact defaultPlannedDate="2026-09-29" onCreated={() => {}} />,
    );
    expect(html).toContain('aria-label="新任务选项"');
    expect(html).toContain('aria-expanded="false"');
  });

  it("已完成任务在悬浮窗中默认折叠，待办保持可见", () => {
    const tasks = [
      { id: "done", title: "已完成事项", status: "done" },
      { id: "todo", title: "待办事项", status: "todo" },
    ] as Task[];
    const html = renderToStaticMarkup(
      <FloatingTaskSections
        tasks={tasks}
        hideDone={false}
        renderTask={(task) => <li key={task.id}>{task.title}</li>}
      />,
    );

    expect(html).toContain(
      '<ul class="floating__list" aria-label="待处理任务">',
    );
    expect(html).toContain('<details class="floating__completed">');
    expect(html).toContain("<summary>已完成（1 项）</summary>");
    expect(html).not.toContain('<details class="floating__completed" open');
    expect(html.indexOf("待办事项")).toBeLessThan(
      html.indexOf("已完成（1 项）"),
    );
  });

  it("手动隐藏选项仍可隐藏整个已完成分组", () => {
    const html = renderToStaticMarkup(
      <FloatingTaskSections
        tasks={[{ id: "done", title: "已完成事项", status: "done" } as Task]}
        hideDone
        renderTask={(task) => <li key={task.id}>{task.title}</li>}
      />,
    );

    expect(html).not.toContain("已完成事项");
    expect(html).not.toContain("floating__completed");
  });

  it("查看历史日期时，之后完成的任务不会被隐藏", () => {
    const html = renderToStaticMarkup(
      <FloatingTaskSections tasks={[{ id: "later", title: "历史待办", status: "done" } as Task]}
        hideDone isCompleted={() => false} renderTask={(task) => <li key={task.id}>{task.title}</li>} />,
    );
    expect(html).toContain("历史待办");
    expect(html).toContain('aria-label="待处理任务"');
  });
});
