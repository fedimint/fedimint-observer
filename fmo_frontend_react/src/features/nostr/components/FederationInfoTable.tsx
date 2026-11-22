import type { ReactNode } from 'react';
import type { FederationInfo } from '../hooks/useFederationInspector';

function Row({ label, checking, alignTop = false, children }: { label: string; checking: boolean; alignTop?: boolean; children: ReactNode }) {
  return (
    <tr className="bg-surface border-b">
      <th scope="row" className={`px-3 sm:px-6 py-3 sm:py-4 font-medium text-ink w-24 sm:w-auto${alignTop ? ' align-top' : ''}`}>
        {label}
      </th>
      <td className="px-3 sm:px-6 py-3 sm:py-4 wrap-break-word">
        {checking ? <div className="h-2.5 bg-surface-3 rounded-full w-32 sm:w-48"></div> : children}
      </td>
    </tr>
  );
}

/** Name, guardian count, modules and network of a checked federation, with placeholders while checking */
export function FederationInfoTable({ info, checking }: { info: FederationInfo | null; checking: boolean }) {
  return (
    <div className="flow-root mt-4">
      <div className="relative">
        <table className="w-full text-xs sm:text-sm text-left rtl:text-right text-ink-3">
          <tbody>
            <Row label="Name" checking={checking}>{info?.name || 'Unknown'}</Row>
            <Row label="Guardians" checking={checking}>{info?.guardians.length || 0}</Row>
            <Row label="Modules" checking={checking} alignTop>
              <div className="flex flex-wrap gap-1">
                {info?.modules.map((mod) => (
                  <span
                    key={mod}
                    className="bg-accent text-white text-xs font-medium px-2.5 py-0.5 rounded-sm "
                  >
                    {mod}
                  </span>
                ))}
              </div>
            </Row>
            <Row label="Network" checking={checking}>{info?.network || 'Unknown'}</Row>
          </tbody>
        </table>
      </div>
    </div>
  );
}
