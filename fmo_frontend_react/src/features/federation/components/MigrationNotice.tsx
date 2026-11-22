import { useState } from 'react';
import { buttonClass } from '../../../components/ui/controls';
import { Link } from 'react-router-dom';
import { MIGRATION_HELP_URL, showMigrationHelp } from '../../../utils/migration';
import { formatUtcDate, formatUtcDateTime } from '../../../utils/time';

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

  if (closed) {
    return (
      <div className="mb-4 rounded-lg border border-line bg-surface-2 p-4">
        <h2 className="text-base font-semibold text-ink">
          This federation moved to {successorName} on <span title={formatUtcDateTime(expiry)}>{date}</span>
        </h2>
        <p className="mt-1 text-sm text-ink-2">The figures below are the last ones observed.</p>
        <div className="mt-3">
          {successor ? (
            <Link to={`/federations/${successor.id}`} className={buttonClass('secondary', 'md')}>
              View {successor.name}<ArrowIcon />
            </Link>
          ) : (
            <CopyInviteButton invite={successorInvite} className={buttonClass('secondary', 'md')} />
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="mb-4 flex gap-3 rounded-lg border border-accent/30 bg-accent-soft p-4">
      <svg className="mt-0.5 size-5 shrink-0 text-accent-ink" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="11" fill="currentColor" />
        <path d="M12 11v6M12 7.5v.01" stroke="white" strokeWidth="2.4" strokeLinecap="round" />
      </svg>
      <div className="min-w-0">
        <h2 className="text-base font-semibold text-accent-ink">This federation is migrating</h2>
        <p className="mt-1 text-sm text-accent-ink sm:text-base">
          Members should move their funds to <strong>{successorName}</strong> before{' '}
          <strong title={formatUtcDateTime(expiry)}>{date}</strong>.
        </p>
        <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2">
          {successor ? (
            <Link to={`/federations/${successor.id}`} className={buttonClass('primary', 'md')}>
              View {successor.name}<ArrowIcon />
            </Link>
          ) : (
            <CopyInviteButton invite={successorInvite} className={buttonClass('secondary', 'md')} />
          )}
          {showMigrationHelp() && (
            <a href={MIGRATION_HELP_URL} target="_blank" rel="noopener noreferrer" className="inline-flex h-10 items-center text-sm font-medium text-accent-ink underline hover:no-underline">
              Why is this happening?
            </a>
          )}
        </div>
      </div>
    </div>
  );
}
