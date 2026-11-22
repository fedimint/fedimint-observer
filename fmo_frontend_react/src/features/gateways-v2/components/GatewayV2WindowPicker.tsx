import type { GatewayWindow } from '../../../types/api';
import { Segmented } from '../../../components/ui/Segmented';
import { GATEWAY_V2_WINDOWS } from '../lib/gatewaysV2';

const OPTIONS = GATEWAY_V2_WINDOWS.map((value) => ({ value, label: value }));

/** The history window, the same range control as every other chart */
export function GatewayV2WindowPicker({ value, onChange }: { value: GatewayWindow; onChange: (window: GatewayWindow) => void }) {
  return <Segmented label="Gateway history window" options={OPTIONS} value={value} onChange={onChange} />;
}
