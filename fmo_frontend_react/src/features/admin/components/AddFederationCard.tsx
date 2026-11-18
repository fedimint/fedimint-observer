import { useState, type FormEvent } from 'react';
import { Link } from 'react-router-dom';
import { Panel } from '../../../components/ui/Panel';
import { adminApi } from '../../../services/adminApi';
import { inviteProblem } from '../lib/format';
import { errorBox, fieldLabel, input, primaryButton, successBox } from '../styles';
import { useAction } from '../useAction';
import type { SignedInAdmin } from '../useAdminSession';

/** Paste an invite code, and the observer starts following its federation */
export function AddFederationCard({ admin }: { admin: SignedInAdmin }) {
  const [invite, setInvite] = useState('');
  const [problem, setProblem] = useState<string | null>(null);
  const add = useAction((code: string) => adminApi.addFederation(admin.identity, code));

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const code = invite.trim();
    const found = inviteProblem(code);
    setProblem(found);
    if (found) return;
    if (await add.run(code)) setInvite('');
  };

  return (
    <Panel title="Add a federation" className="mt-6">
      <p className="mb-4 text-sm text-gray-600 dark:text-gray-300">
        The observer connects to the federation's guardians and starts collecting its history.
        {admin.session.role === 'delegate' && ' You can add up to 20 federations a day.'}
      </p>
      <form onSubmit={submit} className="flex flex-col gap-2 sm:flex-row sm:items-end">
        <div className="min-w-0 flex-1">
          <label htmlFor="admin-invite" className={fieldLabel}>
            Invite code
          </label>
          <input
            id="admin-invite"
            type="text"
            value={invite}
            onChange={(event) => {
              setInvite(event.target.value);
              setProblem(null);
            }}
            disabled={add.running}
            placeholder="fed1…"
            autoComplete="off"
            spellCheck={false}
            className={`${input} font-mono`}
          />
        </div>
        <button type="submit" disabled={add.running} className={`w-full sm:w-auto ${primaryButton}`}>
          {add.running ? 'Adding…' : 'Add federation'}
        </button>
      </form>

      {(problem ?? add.error) && (
        <div role="alert" className={`mt-4 ${errorBox}`}>
          <span className="font-bold">Could not add it: </span>
          {problem ?? add.error}
        </div>
      )}
      {add.data && !add.running && !add.error && !problem && (
        <div role="status" className={`mt-4 ${successBox}`}>
          <span className="font-bold">Added. </span>
          The observer now follows{' '}
          <Link to={`/federations/${encodeURIComponent(add.data)}`} className="font-medium underline hover:no-underline break-all">
            {add.data}
          </Link>
          . Its history fills in over the next minutes.
        </div>
      )}
    </Panel>
  );
}
