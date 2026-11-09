import { useState } from 'react';
import { Link } from 'react-router-dom';
import { MIGRATION_HELP_URL, showMigrationHelp } from '../utils/migration';
import { formatUtcDate, formatUtcDateTime } from '../utils/time';

interface MigrationNoticeProps {
  expiry: Date;
  /** The successor's invite code, as its guardians published it */
  successorInvite: string;
  /** The successor, when the observer tracks it */
  successor: { id: string; name: string } | null;
}

function ArrowIcon() {
  return (
    <svg className="size-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M5 12h14M13 6l6 6-6 6" />
    </svg>
  );
}

function CopyInviteButton({ invite, className }: { invite: string; className: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(invite);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy invite code:', err);
    }
  };
  return (
    <button type="button" onClick={copy} title={invite} className={className}>
      {copied ? 'Copied' : 'Copy invite code'}
    </button>
  );
}

/** Banner on a federation whose guardians named a successor to move funds to */
export function MigrationNotice({ expiry, successorInvite, successor }: MigrationNoticeProps) {
  const closed = expiry.getTime() <= Date.now();
  const date = formatUtcDate(expiry);
  const successorName = successor?.name ?? 'the new federation';
  const button = 'inline-flex min-h-11 items-center gap-2 rounded-lg px-4 text-sm font-medium sm:text-base';

  if (closed) {
    return (
      <div className="mb-4 rounded-lg border border-gray-200 bg-gray-50 p-4 dark:border-gray-700 dark:bg-gray-800/60">
        <h2 className="text-base font-semibold text-gray-800 dark:text-gray-100">
          This federation moved to {successorName} on <span title={formatUtcDateTime(expiry)}>{date}</span>
        </h2>
        <p className="mt-1 text-sm text-gray-600 dark:text-gray-300">The figures below are the last ones observed.</p>
        <div className="mt-3">
          {successor ? (
            <Link to={`/federations/${successor.id}`} className={`${button} border border-gray-300 bg-white text-gray-800 hover:bg-gray-100 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-100 dark:hover:bg-gray-700`}>
              View {successor.name}<ArrowIcon />
            </Link>
          ) : (
            <CopyInviteButton invite={successorInvite} className={`${button} border border-gray-300 bg-white text-gray-800 hover:bg-gray-100 dark:border-gray-600 dark:bg-gray-800 dark:text-gray-100 dark:hover:bg-gray-700`} />
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="mb-4 flex gap-3 rounded-lg border border-blue-200 bg-blue-50 p-4 dark:border-blue-500/40 dark:bg-blue-900/20">
      <svg className="mt-0.5 size-5 shrink-0 text-blue-600 dark:text-blue-400" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="11" fill="currentColor" />
        <path d="M12 11v6M12 7.5v.01" stroke="white" strokeWidth="2.4" strokeLinecap="round" />
      </svg>
      <div className="min-w-0">
        <h2 className="text-base font-semibold text-blue-900 dark:text-blue-200">This federation is migrating</h2>
        <p className="mt-1 text-sm text-blue-800 sm:text-base dark:text-blue-300">
          Members should move their funds to <strong>{successorName}</strong> before{' '}
          <strong title={formatUtcDateTime(expiry)}>{date}</strong>.
        </p>
        <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2">
          {successor ? (
            <Link to={`/federations/${successor.id}`} className={`${button} bg-blue-700 text-white hover:bg-blue-800 dark:bg-blue-600 dark:hover:bg-blue-700`}>
              View {successor.name}<ArrowIcon />
            </Link>
          ) : (
            <CopyInviteButton invite={successorInvite} className={`${button} border border-blue-200 bg-white text-blue-900 hover:bg-blue-100 dark:border-blue-500/40 dark:bg-gray-800 dark:text-blue-200 dark:hover:bg-gray-700`} />
          )}
          {showMigrationHelp() && (
            <a href={MIGRATION_HELP_URL} target="_blank" rel="noopener noreferrer" className="inline-flex min-h-11 items-center text-sm font-medium text-blue-700 underline hover:no-underline sm:text-base dark:text-blue-400">
              Why is this happening?
            </a>
          )}
        </div>
      </div>
    </div>
  );
}
