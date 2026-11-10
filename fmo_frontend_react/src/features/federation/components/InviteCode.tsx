import { QRCodeSVG } from 'qrcode.react';
import { Copyable } from '../../../components/ui/Copyable';

/** A federation invite code as a QR code to scan, with the code to copy below */
export function InviteCode({ invite }: { invite: string }) {
  return (
    <div className="flex flex-col items-center bg-white dark:bg-gray-900 p-3 rounded-lg border border-gray-200 dark:border-gray-600">
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
