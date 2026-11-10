import { Modal } from '../../../components/ui/Modal';

interface RawAnnouncementDialogProps {
  gatewayName: string;
  raw: Record<string, unknown>;
  onClose: () => void;
}

/** A gateway's announcement exactly as the federation published it */
export function RawAnnouncementDialog({ gatewayName, raw, onClose }: RawAnnouncementDialogProps) {
  return (
    <Modal title="Raw announcement" subtitle={gatewayName} onClose={onClose}>
      <pre className="m-0 max-h-[68vh] overflow-auto bg-slate-950 p-5 text-xs leading-5 text-slate-100">
        {JSON.stringify(raw, null, 2)}
      </pre>
    </Modal>
  );
}
