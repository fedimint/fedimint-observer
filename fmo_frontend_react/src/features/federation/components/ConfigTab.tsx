import { useState } from 'react';
import type { ClientConfig } from '../../../types/api';
import { Button } from '../../../components/ui/Button';
import { Card, CardHeader } from '../../../components/ui/Card';
import { Skeleton } from '../../../components/ui/States';

/** The federation's client config as formatted JSON, in its own scroll area so the page keeps its length */
export function ConfigTab({ config }: { config: ClientConfig | undefined }) {
  const [copied, setCopied] = useState(false);
  const json = config ? JSON.stringify(config, null, 2) : '';

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(json);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error('Failed to copy config:', err);
    }
  };

  return (
    <Card aria-labelledby="config-title">
      <CardHeader
        id="config-title"
        title="Client config"
        description="The configuration this federation gives its clients, as JSON"
        actions={config && <Button icon={copied ? 'check' : 'copy'} onClick={copy}>{copied ? 'Copied' : 'Copy JSON'}</Button>}
      />
      {config ? (
        <pre
          tabIndex={0}
          aria-label="Client config JSON"
          className="max-h-[min(70vh,40rem)] overflow-auto rounded-lg border border-line bg-surface-2 p-4 font-mono text-xs leading-relaxed text-ink-2 scrollbar-thin"
        >
          {json}
        </pre>
      ) : (
        <div role="status" aria-label="Loading config"><Skeleton className="h-96 w-full rounded-lg" /></div>
      )}
    </Card>
  );
}
