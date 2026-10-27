import type { ReactNode } from "react";
import type { TaskStatus } from "../lib/types";

type TaskWithStatus = { id: string; status: TaskStatus };

/** Keeps unfinished work visible while completed items remain available in a collapsed section. */
export function TaskSections<T extends TaskWithStatus>({
  tasks,
  separateCompleted = false,
  isCompleted = (task) => task.status === "done",
  renderTask,
}: {
  tasks: T[];
  separateCompleted?: boolean;
  isCompleted?: (task: T) => boolean;
  renderTask: (task: T) => ReactNode;
}) {
  if (!separateCompleted) {
    return <ul className="tasklist">{tasks.map(renderTask)}</ul>;
  }

  const unfinished = tasks.filter((task) => !isCompleted(task));
  const completed = tasks.filter(isCompleted);

  return (
    <div className="tasklist-sections">
      {unfinished.length > 0 && (
        <ul className="tasklist" aria-label="待处理任务">
          {unfinished.map(renderTask)}
        </ul>
      )}
      {completed.length > 0 && (
        <details className="tasklist-completed">
          <summary>已完成（{completed.length} 项）</summary>
          <ul className="tasklist" aria-label="已完成任务">
            {completed.map(renderTask)}
          </ul>
        </details>
      )}
    </div>
  );
}
