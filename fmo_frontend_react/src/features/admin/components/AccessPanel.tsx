import { useState, type FormEvent } from 'react';
import { ListCard } from '../../../components/ui/ListCard';
import { Panel } from '../../../components/ui/Panel';
import { RetryButton } from '../../../components/ui/RetryButton';
import { adminApi } from '../../../services/adminApi';
import type { AdminDelegate, AdminKeys } from '../../../types/api';
import { formatUtcDate, formatUtcDateTime } from '../../../utils/time';
import { GRANT_DURATIONS, keyProblem } from '../lib/format';
import { dangerButton, errorBox, fieldLabel, input, primaryButton, quietButton } from '../styles';
import { useAction } from '../useAction';
import type { SignedInAdmin } from '../useAdminSession';
import { ShortKey } from './ShortKey';

const DAY_MS = 24 * 60 * 60 * 1000;

interface AccessPanelProps {
  admin: SignedInAdmin;
  keys: AdminKeys | null;
  loading: boolean;
  error: string | null;
  onReload: () => void;
  /** The list after a grant or revoke, so it needs no further request */
  onChange: (update: (keys: AdminKeys) => AdminKeys) => void;
}

/** Owners only: who has access, granting it and taking it back */
export function AccessPanel({ admin, keys, loading, error, onReload, onChange }: AccessPanelProps) {
  return (
    <>
      <GrantForm
        admin={admin}
        full={keys !== null && keys.delegates.length >= keys.max_delegates}
        onGranted={(granted) =>
          onChange((current) => ({
            ...current,
            delegates: [granted, ...current.delegates.filter((delegate) => delegate.key.pubkey !== granted.key.pubkey)],
          }))
        }
      />

      <ListCard className="mt-6">
        <h2 className="px-4 py-4 text-base font-semibold text-gray-900 sm:px-6 sm:text-lg dark:text-white">People with access</h2>
        <div className="divide-y divide-gray-200 border-t border-gray-200 dark:divide-gray-700 dark:border-gray-700">
          {error ? (
            <div role="alert" className="px-4 py-4 text-sm text-red-800 sm:px-6 dark:text-red-400">
              <span className="font-bold">Could not load who has access: </span>
              {error} <RetryButton onClick={onReload} />
            </div>
          ) : !keys || loading ? (
            <p role="status" className="px-4 py-4 text-sm text-gray-500 sm:px-6 dark:text-gray-400">
              {loading ? 'Waiting for your signature, then loading…' : 'Not loaded yet.'}
            </p>
          ) : (
            <>
              {keys.owners.map((owner) => (
                <div key={owner.pubkey} className="flex flex-wrap items-center justify-between gap-2 px-4 py-3 sm:px-6">
                  <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1">
                    <span className="text-sm font-medium text-gray-900 dark:text-white">Owner</span>
                    <ShortKey id={owner} />
                    {owner.pubkey === admin.identity.pubkey && <span className="text-xs text-gray-500 dark:text-gray-400">you</span>}
                  </div>
                  <span className="text-xs text-gray-500 dark:text-gray-400">set in the server's configuration</span>
                </div>
              ))}
              {keys.delegates.map((delegate) => (
                <DelegateRow
                  key={delegate.key.pubkey}
                  admin={admin}
                  delegate={delegate}
                  onRevoked={() =>
                    onChange((current) => ({
                      ...current,
                      delegates: current.delegates.filter((other) => other.key.pubkey !== delegate.key.pubkey),
                    }))
                  }
                />
              ))}
              {keys.delegates.length === 0 && (
                <p className="px-4 py-4 text-sm text-gray-500 sm:px-6 dark:text-gray-400">
                  Nobody else has access yet. Grant it to a public key above.
                </p>
              )}
            </>
          )}
        </div>
      </ListCard>
    </>
  );
}

function GrantForm({ admin, full, onGranted }: { admin: SignedInAdmin; full: boolean; onGranted: (delegate: AdminDelegate) => void }) {
  const [key, setKey] = useState('');
  const [label, setLabel] = useState('');
  const [days, setDays] = useState<number | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const grant = useAction((pubkey: string, name: string, lasting: number | null) =>
    adminApi.grantKey(admin.identity, pubkey, {
      label: name,
      ...(lasting === null ? {} : { expires_at: new Date(Date.now() + lasting * DAY_MS).toISOString() }),
    }),
  );

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const pubkey = key.trim();
    const name = label.trim();
    const found = keyProblem(pubkey) ?? (name ? null : 'Give the person a name, so you recognise the key later.');
    setProblem(found);
    if (found) return;
    const granted = await grant.run(pubkey, name, days);
    if (granted) {
      onGranted(granted);
      setKey('');
      setLabel('');
    }
  };

  return (
    <Panel title="Grant access" className="mt-6">
      <p className="mb-4 text-sm text-gray-600 dark:text-gray-300">
        The person signs in here with their own Nostr key and can add federations. They can't manage access or run
        maintenance, and you can take the access back at any time.
      </p>
      <form onSubmit={submit} className="grid grid-cols-1 gap-3 sm:grid-cols-[minmax(0,2fr)_minmax(0,1fr)_auto_auto] sm:items-end">
        <div className="min-w-0">
          <label htmlFor="grant-key" className={fieldLabel}>
            Their public key
          </label>
          <input
            id="grant-key"
            type="text"
            value={key}
            onChange={(event) => {
              setKey(event.target.value);
              setProblem(null);
            }}
            disabled={grant.running}
            placeholder="npub1…"
            autoComplete="off"
            spellCheck={false}
            className={`${input} font-mono`}
          />
        </div>
        <div className="min-w-0">
          <label htmlFor="grant-label" className={fieldLabel}>
            Name
          </label>
          <input
            id="grant-label"
            type="text"
            value={label}
            onChange={(event) => {
              setLabel(event.target.value);
              setProblem(null);
            }}
            disabled={grant.running}
            maxLength={64}
            placeholder="Alice"
            autoComplete="off"
            className={input}
          />
        </div>
        <div>
          <label htmlFor="grant-duration" className={fieldLabel}>
            Lasts
          </label>
          <select
            id="grant-duration"
            value={days ?? ''}
            onChange={(event) => setDays(event.target.value === '' ? null : Number(event.target.value))}
            disabled={grant.running}
            className={input}
          >
            {GRANT_DURATIONS.map((duration) => (
              <option key={duration.label} value={duration.days ?? ''}>
                {duration.label}
              </option>
            ))}
          </select>
        </div>
        <button type="submit" disabled={grant.running || full} className={`w-full sm:w-auto ${primaryButton}`}>
          {grant.running ? 'Granting…' : 'Grant access'}
        </button>
      </form>
      {full && <p className="mt-3 text-xs text-gray-500 dark:text-gray-400">The limit of keys with access is reached. Revoke one to grant another.</p>}
      {(problem ?? grant.error) && (
        <div role="alert" className={`mt-4 ${errorBox}`}>
          <span className="font-bold">Could not grant access: </span>
          {problem ?? grant.error}
        </div>
      )}
    </Panel>
  );
}

function DelegateRow({ admin, delegate, onRevoked }: { admin: SignedInAdmin; delegate: AdminDelegate; onRevoked: () => void }) {
  const [confirming, setConfirming] = useState(false);
  const revoke = useAction(async () => {
    await adminApi.revokeKey(admin.identity, delegate.key.pubkey);
    return true;
  });

  const confirm = async () => {
    if (await revoke.run()) onRevoked();
    else setConfirming(false);
  };

  return (
    <div className="px-4 py-3 sm:px-6">
      <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
            {/* A label is whatever an owner typed: shown as text only */}
            <span className="text-sm font-medium text-gray-900 break-all dark:text-white">{delegate.label}</span>
            <ShortKey id={delegate.key} />
            {delegate.expired && (
              <span className="rounded-sm bg-yellow-100 px-2 py-0.5 text-xs font-medium text-yellow-800 dark:bg-yellow-900 dark:text-yellow-300">
                expired
              </span>
            )}
          </div>
          <p className="mt-1 text-xs text-gray-500 dark:text-gray-400">
            granted {formatUtcDate(delegate.granted_at)}
            {' · '}
            {delegate.expires_at ? `${delegate.expired ? 'ended' : 'until'} ${formatUtcDateTime(delegate.expires_at)}` : 'until revoked'}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {confirming ? (
            <>
              <button type="button" onClick={() => void confirm()} disabled={revoke.running} className={dangerButton}>
                {revoke.running ? 'Revoking…' : 'Yes, revoke'}
              </button>
              <button type="button" onClick={() => setConfirming(false)} disabled={revoke.running} className={quietButton}>
                Keep
              </button>
            </>
          ) : (
            <button type="button" onClick={() => setConfirming(true)} aria-label={`Revoke ${delegate.label}`} className={dangerButton}>
              Revoke
            </button>
          )}
        </div>
      </div>
      {revoke.error && (
        <div role="alert" className={`mt-2 ${errorBox}`}>
          <span className="font-bold">Could not revoke: </span>
          {revoke.error}
        </div>
      )}
    </div>
  );
}
