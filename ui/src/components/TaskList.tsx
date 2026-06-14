// components/TaskList.tsx — Claude Code TaskListV2 1:1
// Source: docs/27-todo-tool-ui.md
// Renders below the Spinner with task status icons, priority sorting, and collapse summary.
import React from 'react';
import { Box, Text } from 'ink';
import { theme } from '../theme.js';
import type { TaskItem } from '../core/types.js';

interface TaskListProps {
  tasks: TaskItem[];
}

const MAX_DISPLAY = 10;

function sortTasks(tasks: TaskItem[]): TaskItem[] {
  const unresolvedIds = new Set(
    tasks.filter(t => t.status !== 'completed').map(t => t.id)
  );
  const isBlocked = (t: TaskItem) =>
    (t.blocked_by || []).some(id => unresolvedIds.has(id));

  return [...tasks].sort((a, b) => {
    // in_progress first
    if (a.status === 'in_progress' && b.status !== 'in_progress') return -1;
    if (b.status === 'in_progress' && a.status !== 'in_progress') return 1;
    // pending before completed
    if (a.status === 'pending' && b.status === 'completed') return -1;
    if (b.status === 'pending' && a.status === 'completed') return 1;
    // among pending: unblocked before blocked
    if (a.status === 'pending' && b.status === 'pending') {
      const ab = isBlocked(a), bb = isBlocked(b);
      if (ab !== bb) return ab ? 1 : -1;
    }
    // tie-break by id ascending
    return a.id - b.id;
  });
}

function getTaskIcon(status: TaskItem['status']): { icon: string; color: string } {
  switch (status) {
    case 'completed':   return { icon: '✔', color: theme.success };
    case 'in_progress': return { icon: '◼', color: theme.claude };
    case 'pending':     return { icon: '◻', color: theme.text };
  }
}

export function TaskList({ tasks }: TaskListProps): React.ReactElement {
  const sorted = sortTasks(tasks);
  const visible = sorted.slice(0, MAX_DISPLAY);
  const hidden = sorted.slice(MAX_DISPLAY);

  // Build hidden summary string
  const hiddenParts: string[] = [];
  if (hidden.length > 0) {
    const ip = hidden.filter(t => t.status === 'in_progress').length;
    const pd = hidden.filter(t => t.status === 'pending').length;
    const cp = hidden.filter(t => t.status === 'completed').length;
    if (ip > 0) hiddenParts.push(`${ip} in progress`);
    if (pd > 0) hiddenParts.push(`${pd} pending`);
    if (cp > 0) hiddenParts.push(`${cp} completed`);
  }

  const unresolvedIds = new Set(
    tasks.filter(t => t.status !== 'completed').map(t => t.id)
  );

  return (
    <Box flexDirection="column" marginTop={1} paddingLeft={2} width="100%">
      {visible.map(task => {
        const { icon, color } = getTaskIcon(task.status);
        const isCompleted = task.status === 'completed';
        const isInProgress = task.status === 'in_progress';
        const openBlockers = (task.blocked_by || []).filter(id => unresolvedIds.has(id));
        const isBlocked = openBlockers.length > 0;

        return (
          <Box key={task.id} flexDirection="column">
            <Box>
              <Text color={color}>{icon} </Text>
              <Text
                bold={isInProgress}
                color={isCompleted || isBlocked ? theme.inactive : theme.text}
              >
                {task.subject}
              </Text>
              {isBlocked && (
                <Text color={theme.inactive}> › blocked by {openBlockers.map(id => `#${id}`).join(', ')}</Text>
              )}
            </Box>
          </Box>
        );
      })}
      {hiddenParts.length > 0 && (
        <Text color={theme.inactive}> … +{hiddenParts.join(', ')}</Text>
      )}
    </Box>
  );
}
