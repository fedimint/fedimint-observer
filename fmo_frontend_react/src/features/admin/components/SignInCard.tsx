import { Copyable } from '../../../components/ui/Copyable';
import { Panel } from '../../../components/ui/Panel';
import { errorBox, primaryButton } from '../styles';
import { useAdminSession } from '../useAdminSession';

/** What a visitor without a session sees: how signing in works, and the button */
export function SignInCard() {
  const { signIn, signingIn, failure } = useAdminSession();

  return (
    <Panel title="Sign in" className="mt-6">
      <p className="text-sm text-gray-600 dark:text-gray-300">
        Admins sign in with a Nostr key held by a browser extension such as nos2x or Alby. Your key never leaves the
        extension: it signs each admin request, and this page stores nothing.
      </p>
      <button type="button" onClick={() => void signIn()} disabled={signingIn} className={`mt-4 w-full sm:w-auto ${primaryButton}`}>
        {signingIn ? 'Waiting for your extension…' : 'Sign in with Nostr'}
      </button>

      {failure && (
        <div role="alert" className={`mt-4 ${errorBox}`}>
          <span className="font-bold">{failure.title}: </span>
          {failure.message}
        </div>
      )}
      {failure?.refusedKey && (
        <div className="mt-4">
          <p className="mb-2 text-xs text-gray-600 sm:text-sm dark:text-gray-300">
            To get access, send this public key to an owner of this observer:
          </p>
          <Copyable text={failure.refusedKey} />
        </div>
      )}
    </Panel>
  );
}
