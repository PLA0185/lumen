import type { ReactNode } from "react";
import type { Task } from "../lib/types";

/** Keeps unfinished work visible while completed tasks stay available in a collapsed group. */
export function FloatingTaskSections({
  tasks,
  hideDone,
  isCompleted = (task) => task.status === "done",
  renderTask,
}: {
  tasks: Task[];
  hideDone: boolean;
  isCompleted?: (task: Task) => boolean;
  renderTask: (task: Task) => ReactNode;
}) {
  const unfinished = tasks.filter((task) => !isCompleted(task));
  const completed = tasks.filter(isCompleted);

  return (
    <>
      {unfinished.length > 0 && (
        <ul className="floating__list" aria-label="待处理任务">
          {unfinished.map(renderTask)}
        </ul>
      )}
      {!hideDone && completed.length > 0 && (
        <details className="floating__completed">
          <summary>已完成（{completed.length} 项）</summary>
          <ul className="floating__list" aria-label="已完成任务">
            {completed.map(renderTask)}
          </ul>
        </details>
      )}
    </>
  );
}
