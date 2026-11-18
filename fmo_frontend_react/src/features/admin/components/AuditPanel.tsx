import { Link } from 'react-router-dom';
import { ListCard } from '../../../components/ui/ListCard';
import { RetryButton } from '../../../components/ui/RetryButton';
import type { AdminAuditEntry } from '../../../types/api';
import { formatUtcDateTime } from '../../../utils/time';
import { actionLabel, detailText, isFederationAction, shortKey } from '../lib/format';
import { quietButton } from '../styles';
import { RoleBadge } from './RoleBadge';
import { ShortKey } from './ShortKey';

const outcomeStyles: Record<AdminAuditEntry['outcome'], string> = {
  succeeded: 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-300',
  failed: 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-300',
  // Recorded before the action ran, with no outcome after: the request was cut
  // off, so the action may or may not have happened
  started: 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-300',
};

interface AuditPanelProps {
  entries: AdminAuditEntry[] | null;
  /** Whether older entries exist */
  hasOlder: boolean;
  loading: boolean;
  error: string | null;
  onLoadOlder: () => void;
  onReload: () => void;
}

/** Owners only: what admins did, newest first */
export function AuditPanel({ entries, hasOlder, loading, error, onLoadOlder, onReload }: AuditPanelProps) {
  return (
    <ListCard className="mt-6">
      <div className="flex items-center justify-between gap-3 px-4 py-4 sm:px-6">
        <div>
          <h2 className="text-base font-semibold text-gray-900 sm:text-lg dark:text-white">Activity</h2>
          <p className="mt-1 text-xs text-gray-500 sm:text-sm dark:text-gray-400">Every admin action, recorded before it ran</p>
        </div>
        <button type="button" onClick={onReload} disabled={loading} className={quietButton}>
          Refresh
        </button>
      </div>
      <div className="divide-y divide-gray-200 border-t border-gray-200 dark:divide-gray-700 dark:border-gray-700">
        {entries?.map((entry) => <AuditRow key={entry.id} entry={entry} />)}
        {entries?.length === 0 && !loading && !error && (
          <p className="px-4 py-4 text-sm text-gray-500 sm:px-6 dark:text-gray-400">Nothing recorded yet.</p>
        )}
        {error && (
          <div role="alert" className="px-4 py-4 text-sm text-red-800 sm:px-6 dark:text-red-400">
            <span className="font-bold">Could not load the activity: </span>
            {error} <RetryButton onClick={entries ? onLoadOlder : onReload} />
          </div>
        )}
        {loading && (
          <p role="status" className="px-4 py-4 text-sm text-gray-500 sm:px-6 dark:text-gray-400">
            Waiting for your signature, then loading…
          </p>
        )}
        {entries && hasOlder && !loading && !error && (
          <div className="px-4 py-3 text-center sm:px-6">
            <button type="button" onClick={onLoadOlder} className={quietButton}>
              Load older
            </button>
          </div>
        )}
      </div>
    </ListCard>
  );
}

function AuditRow({ entry }: { entry: AdminAuditEntry }) {
  const label = detailText(entry, 'label');
  return (
    <div className="px-4 py-3 text-sm sm:px-6">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <span className="font-medium text-gray-900 dark:text-white">{actionLabel(entry.action)}</span>
        {isFederationAction(entry) ? (
          <Link
            to={`/federations/${encodeURIComponent(entry.subject)}`}
            title={entry.subject}
            className="font-mono text-xs text-blue-700 hover:underline dark:text-blue-400"
          >
            {shortKey(entry.subject)}
          </Link>
        ) : (
          <>
            {/* Details are free-form: shown as text only */}
            {label && <span className="text-gray-700 break-all dark:text-gray-300">{label}</span>}
            {entry.subject_key ? (
              <ShortKey id={entry.subject_key} />
            ) : (
              <span title={entry.subject} className="font-mono text-xs text-gray-700 dark:text-gray-300">
                {shortKey(entry.subject)}
              </span>
            )}
          </>
        )}
        <span
          title={entry.outcome === 'started' ? 'The request ended before its outcome was stored. Check the actual state before trying again.' : undefined}
          className={`rounded-sm px-2 py-0.5 text-xs font-medium ${outcomeStyles[entry.outcome] ?? outcomeStyles.started}`}
        >
          {entry.outcome === 'started' ? 'outcome not recorded' : entry.outcome}
        </span>
      </div>
      <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-gray-500 dark:text-gray-400">
        <time dateTime={entry.at}>{formatUtcDateTime(entry.at)}</time>
        <span>by</span>
        {entry.actor ? <ShortKey id={entry.actor} /> : <span>the server token</span>}
        <RoleBadge role={entry.actor_role} />
      </div>
    </div>
  );
}
