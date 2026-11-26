import type { FormEvent } from 'react';
import { Button } from '../../../components/ui/Button';
import { TextInput } from '../../../components/ui/TextInput';
import { ListCard } from '../../../components/ui/ListCard';
import { useFederationInspector } from '../hooks/useFederationInspector';
import { FederationInfoTable } from './FederationInfoTable';

/** Checks a federation by invite code, and announces it on Nostr once checked */
export function InspectFederationCard() {
  const inspector = useFederationInspector();
  const { inviteCode, checking, info, checkedInfo, error, announcing, announced } = inspector;

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    void inspector.check();
  };

  return (
    <ListCard className="mt-8">
      <div className="p-4 sm:p-5">
        <h1 className="text-base font-medium text-ink">Inspect federation</h1>
        <p className="mt-0.5 text-xs text-ink-3 sm:text-sm">Fetch federation info by invite code</p>
      </div>

      <div className="p-4 pt-0 sm:p-5 sm:pt-0">
        <form className="flex flex-col sm:flex-row gap-2 sm:items-center" onSubmit={handleSubmit}>
          <TextInput
            size="md"
            value={inviteCode}
            onChange={(e) => inspector.changeInviteCode(e.target.value)}
            disabled={checking || announcing}
            placeholder="Invite code (fed1…)"
            aria-label="Invite code"
            className="w-full flex-1 font-mono"
          />
          <div className="flex flex-col sm:flex-row gap-2 w-full sm:w-auto">
            <Button type="submit" variant="primary" size="md" disabled={checking} className="w-full sm:w-auto">
              {checking ? 'Checking…' : 'Check federation'}
            </Button>
            <Button
              size="md"
              icon={announced ? 'check' : 'nostr'}
              onClick={inspector.announce}
              disabled={!checkedInfo || announcing || announced || checking}
              title={checkedInfo ? undefined : 'Check the federation first'}
              className="w-full sm:w-auto"
            >
              {announcing ? 'Announcing…' : announced ? 'Announced' : 'Announce on Nostr'}
            </Button>
          </div>
        </form>

        {(checking || info) && <FederationInfoTable info={info} checking={checking} />}

        {error && (
          <div role="alert" className="mt-4 rounded-xl border border-crit/25 bg-crit-soft px-4 py-3 text-sm text-crit-ink wrap-break-word">
            <span className="font-medium">Error: </span>
            {error}
          </div>
        )}

        {announced && (
          <div className="p-3 sm:p-4 mb-4 mt-4 text-xs sm:text-sm text-ok-ink rounded-lg bg-ok-soft">
            Federation announced. Reload the page to see it listed.
          </div>
        )}
      </div>
    </ListCard>
  );
}
