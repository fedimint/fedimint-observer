import type { AdminRole } from '../../../types/api';

const styles: Record<AdminRole, string> = {
  owner: 'bg-indigo-100 text-indigo-800 dark:bg-indigo-900 dark:text-indigo-300',
  delegate: 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-300',
};

export function RoleBadge({ role }: { role: AdminRole }) {
  return <span className={`rounded-sm px-2 py-0.5 text-xs font-medium capitalize ${styles[role]}`}>{role}</span>;
}
