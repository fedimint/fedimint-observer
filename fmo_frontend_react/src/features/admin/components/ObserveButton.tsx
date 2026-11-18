import { Link } from 'react-router-dom';
import { adminApi } from '../../../services/adminApi';
import { quietButton } from '../styles';
import { useAction } from '../useAction';
import { useAdminSession } from '../useAdminSession';

/**
 * For signed-in admins, next to a federation the observer doesn't follow yet:
 * adds it from the invite code the list already has. Renders nothing for
 * everyone else.
 */
export function ObserveButton({ invite }: { invite: string }) {
  const { admin } = useAdminSession();
  const add = useAction(async () => {
    if (!admin) throw new Error('Sign in as an admin first');
    return adminApi.addFederation(admin.identity, invite);
  });
  if (!admin) return null;

  if (add.data) {
    return (
      <Link to={`/federations/${encodeURIComponent(add.data)}`} className="text-xs font-medium text-green-700 underline hover:no-underline dark:text-green-400">
        Added, open it
      </Link>
    );
  }
  return (
    <span className="inline-flex flex-wrap items-center gap-2">
      <button type="button" onClick={() => void add.run()} disabled={add.running} className={quietButton}>
        {add.running ? 'Adding…' : 'Observe'}
      </button>
      {add.error && (
        <span role="alert" className="text-xs font-normal text-red-700 dark:text-red-400">
          {add.error}
        </span>
      )}
    </span>
  );
}
