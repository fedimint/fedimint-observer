import { QRCodeSVG } from 'qrcode.react';
import { Copyable } from '../../../components/ui/Copyable';

/** A federation invite code as a QR code to scan, with the code to copy below */
export function InviteCode({ invite }: { invite: string }) {
  return (
    <div className="flex flex-col items-center bg-surface p-3 rounded-lg border border-line">
      <QRCodeSVG
        value={invite}
        size={160}
        level="M"
        marginSize={4}
        className="w-full h-auto max-w-[160px]"
      />
      <div className="mt-3 w-full">
        <Copyable text={invite} />
      </div>
    </div>
  );
}
