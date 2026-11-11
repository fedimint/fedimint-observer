import { useState, type ComponentProps } from 'react';
import { ListCard, ListMessage } from '../../../components/ui/ListCard';
import { ChevronUpIcon } from '../../../components/ui/icons';
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
  const [collapsed, setCollapsed] = useState(true);

  return (
    <ListCard className="mt-6">
      <button
        type="button"
        aria-expanded={!collapsed}
        aria-controls={id}
        className="w-full p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800"
        onClick={() => setCollapsed(!collapsed)}
      >
        <ChevronUpIcon className={`w-3 h-3 shrink-0 inline mr-2 transition-transform ${collapsed ? 'rotate-180' : ''}`} />
        <span>{title}</span>
        {!loading && !failed && total !== null && (
          <span className="ml-2 text-sm font-normal text-gray-500 dark:text-gray-400">({total})</span>
        )}
        <span className="mt-1 block text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
          {description}
        </span>
      </button>
      <div id={id} hidden={collapsed}>
        {!collapsed && (
          <FederationList
            {...listProps}
            federations={federations}
            failed={failed}
            loading={loading ? <ListMessage>Loading...</ListMessage> : null}
          />
        )}
      </div>
    </ListCard>
  );
}
