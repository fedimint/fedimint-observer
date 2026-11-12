import type { ClientConfig } from '../../../types/api';
import { Panel } from '../../../components/ui/Panel';

/** The federation's client config as formatted JSON */
export function ConfigTab({ config }: { config: ClientConfig | undefined }) {
  return (
    <Panel padding="p-3 sm:p-6" className="overflow-hidden">
      <pre className="text-[10px] sm:text-xs lg:text-sm text-gray-900 dark:text-white overflow-x-auto whitespace-pre wrap-break-word">
        {config ? JSON.stringify(config, null, 2) : 'Loading config...'}
      </pre>
    </Panel>
  );
}
