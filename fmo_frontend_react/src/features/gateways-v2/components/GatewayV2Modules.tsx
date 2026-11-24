import { useState } from 'react';
import { Select } from '../../../components/ui/Select';
import type { GatewayWindow } from '../../../types/api';
import { GatewayV2Directory } from './GatewayV2Directory';

interface GatewayV2ModulesProps {
  id: string;

  instances: number[];
  timeWindow: GatewayWindow;
}

/** The gateways of one Lightning v2 module, with a picker when the federation has several */
export function GatewayV2Modules({ id, instances, timeWindow }: GatewayV2ModulesProps) {
  const [instance, setInstance] = useState(instances[0]);
  return (
    <>
      {instances.length > 1 && (
        <label className="mb-4 inline-flex items-center gap-2 text-xs text-ink-3">
          Lightning module
          <Select value={instance} onChange={(event) => setInstance(Number(event.target.value))}>
            {instances.map((id) => <option key={id} value={id}>Lightning #{id}</option>)}
          </Select>
        </label>
      )}
      <GatewayV2Directory key={`${id}:${instance}:${timeWindow}`} id={id} instance={instance} timeWindow={timeWindow} />
    </>
  );
}
