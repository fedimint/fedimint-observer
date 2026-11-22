import type { ComponentProps } from 'react';
import { CollapsibleCard } from '../../../components/ui/CollapsibleCard';
import { ListMessage } from '../../../components/ui/ListCard';
import { FederationList } from './FederationList';

interface CollapsibleFederationsProps extends Omit<ComponentProps<typeof FederationList>, 'loading'> {
  id: string;
  title: string;
  description: string;
  loading: boolean;
  /** Federations in this group across all pages */
  total: number | null;
}

/** A secondary federation list, collapsed until opened */
export function CollapsibleFederations({ id, title, description, loading, failed, federations, total, ...listProps }: CollapsibleFederationsProps) {
  return (
    <CollapsibleCard id={id} title={title} description={description} count={!loading && !failed ? total : null}>
      <FederationList
        {...listProps}
        federations={federations}
        failed={failed}
        loading={loading ? <ListMessage>Loading...</ListMessage> : null}
      />
    </CollapsibleCard>
  );
}
