import { useState } from 'react';
import { Tabs, type TabItem } from '../components/ui/Tabs';
import { AccessPanel } from '../features/admin/components/AccessPanel';
import { AddFederationCard } from '../features/admin/components/AddFederationCard';
import { AuditPanel } from '../features/admin/components/AuditPanel';
import { SessionBar } from '../features/admin/components/SessionBar';
import { SignInCard } from '../features/admin/components/SignInCard';
import { useAction } from '../features/admin/useAction';
import { useAdminSession, type SignedInAdmin } from '../features/admin/useAdminSession';
import { adminApi } from '../services/adminApi';
import type { AdminAuditEntry } from '../types/api';

type AdminTab = 'add' | 'access' | 'activity';

const OWNER_TABS: readonly TabItem<AdminTab>[] = [
  { id: 'add', label: 'Add federation' },
  { id: 'access', label: 'Access' },
  { id: 'activity', label: 'Activity' },
];

export function Admin() {
  const { admin } = useAdminSession();

  return (
    <div className="pb-4">
      <h1 className="mt-6 text-xl font-semibold text-gray-900 sm:text-2xl dark:text-white">Admin</h1>
      <p className="mt-1 text-sm text-gray-500 dark:text-gray-400">Add federations to this observer and manage who else can.</p>
      {/* Keyed by key, so one admin's loaded lists never show under another's session */}
      {admin ? <SignedIn key={admin.identity.pubkey} admin={admin} /> : <SignInCard />}
    </div>
  );
}

interface AuditLog {
  entries: AdminAuditEntry[];
  next: number | null;
}

function SignedIn({ admin }: { admin: SignedInAdmin }) {
  const [tab, setTab] = useState<AdminTab>('add');
  const isOwner = admin.session.role === 'owner';

  // Each list asks the signer once, when its tab is first opened, not on render
  const keys = useAction(() => adminApi.listKeys(admin.identity));
  const audit = useAction(async (previous: AuditLog | null): Promise<AuditLog> => {
    const page = await adminApi.auditLog(admin.identity, previous?.next ?? undefined);
    return { entries: [...(previous?.entries ?? []), ...page.entries], next: page.next };
  });

  const select = (next: AdminTab) => {
    setTab(next);
    if (next === 'access' && !keys.data && !keys.running) void keys.run();
    if (next === 'activity' && !audit.data && !audit.running) void audit.run(null);
  };

  return (
    <>
      <SessionBar admin={admin} />
      {isOwner && (
        <div className="mt-6">
          <Tabs items={OWNER_TABS} active={tab} onSelect={select} />
        </div>
      )}
      {(tab === 'add' || !isOwner) && <AddFederationCard admin={admin} />}
      {isOwner && tab === 'access' && (
        <AccessPanel
          admin={admin}
          keys={keys.data}
          loading={keys.running}
          error={keys.error}
          onReload={() => void keys.run()}
          onChange={(update) => keys.setData((current) => (current ? update(current) : current))}
        />
      )}
      {isOwner && tab === 'activity' && (
        <AuditPanel
          entries={audit.data?.entries ?? null}
          hasOlder={audit.data?.next != null}
          loading={audit.running}
          error={audit.error}
          onLoadOlder={() => void audit.run(audit.data)}
          onReload={() => void audit.run(null)}
        />
      )}
    </>
  );
}
