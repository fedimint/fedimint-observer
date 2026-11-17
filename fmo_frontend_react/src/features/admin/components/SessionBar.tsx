import { formatUtcDateTime } from '../../../utils/time';
import { quietButton } from '../styles';
import { useAdminSession, type SignedInAdmin } from '../useAdminSession';
import { RoleBadge } from './RoleBadge';
import { ShortKey } from './ShortKey';

/** Who is signed in, and the way out */
export function SessionBar({ admin }: { admin: SignedInAdmin }) {
  const { signOut } = useAdminSession();
  const { session } = admin;

  return (
    <div className="mt-6 flex flex-wrap items-center justify-between gap-3 rounded-lg border border-gray-200 bg-white p-4 shadow-md dark:border-gray-700 dark:bg-gray-800">
      <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 text-sm text-gray-900 dark:text-white">
        <RoleBadge role={session.role} />
        {session.label && <span className="font-medium break-all">{session.label}</span>}
        {session.key && <ShortKey id={session.key} />}
        {session.expires_at && (
          <span className="text-xs text-gray-500 dark:text-gray-400">access until {formatUtcDateTime(session.expires_at)}</span>
        )}
      </div>
      <button type="button" onClick={signOut} className={quietButton}>
        Sign out
      </button>
    </div>
  );
}
