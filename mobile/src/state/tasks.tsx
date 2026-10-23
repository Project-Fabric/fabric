import { createContext, useContext, useReducer, type ReactNode } from 'react';
import { advanceTask, beginTask, taskExamples, type TaskAction, type TaskState } from '../domain/task-fixtures';

export type TaskMap = Record<string, TaskState>;
type Action = { id: string; action: TaskAction } | { reset: true };
export const initialTasks = (): TaskMap => Object.fromEntries(taskExamples.map(example => [example.id, beginTask(example)]));
export function tasksReducer(tasks: TaskMap, event: Action): TaskMap {
  if ('reset' in event) return initialTasks();
  if (!Object.hasOwn(tasks, event.id)) return tasks;
  const next = advanceTask(tasks[event.id], event.action);
  return next === tasks[event.id] ? tasks : { ...tasks, [event.id]: next };
}
const Context = createContext<{ tasks: TaskMap; update: (id: string, action: TaskAction) => void; reset: () => void } | null>(null);
export function TasksProvider({ children }: { children: ReactNode }) {
  const [tasks, dispatch] = useReducer(tasksReducer, undefined, initialTasks);
  return <Context.Provider value={{ tasks, update: (id, action) => dispatch({ id, action }), reset: () => dispatch({ reset: true }) }}>{children}</Context.Provider>;
}
export function useTasks() {
  const value = useContext(Context);
  if (!value) throw new Error('TasksProvider is required.');
  return value;
}
