import { useState } from 'react';
import type { GatewayWindow } from '../../../types/api';
import { GatewayV2Directory } from './GatewayV2Directory';

interface GatewayV2ModulesProps {
  id: string;
  /** The federation's Lightning v2 module instances; at least one */
  instances: number[];
  timeWindow: GatewayWindow;
}

/** The gateways of one Lightning v2 module, with a picker when the federation has several */
export function GatewayV2Modules({ id, instances, timeWindow }: GatewayV2ModulesProps) {
  const [instance, setInstance] = useState(instances[0]);
  return (
    <>
      {instances.length > 1 && (
        <label className="mb-4 block text-sm">
          Lightning module{' '}
          <select value={instance} onChange={(event) => setInstance(Number(event.target.value))} className="ml-2 rounded-lg border border-gray-300 bg-white p-2 dark:border-gray-600 dark:bg-gray-800">
            {instances.map((id) => <option key={id} value={id}>Lightning #{id}</option>)}
          </select>
        </label>
      )}
      <GatewayV2Directory key={`${id}:${instance}:${timeWindow}`} id={id} instance={instance} timeWindow={timeWindow} />
    </>
  );
}
