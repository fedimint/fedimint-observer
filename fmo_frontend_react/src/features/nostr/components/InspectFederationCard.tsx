import type { FormEvent } from 'react';
import { ListCard } from '../../../components/ui/ListCard';
import { useFederationInspector } from '../hooks/useFederationInspector';
import { FederationInfoTable } from './FederationInfoTable';

/** Checks a federation by invite code, and announces it on Nostr once checked */
export function InspectFederationCard() {
  const inspector = useFederationInspector();
  const { inviteCode, checking, info, checkedInfo, error, announcing, announced } = inspector;

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    void inspector.check();
  };

  return (
    <ListCard className="mt-8">
      <h1 className="p-4 sm:p-5 text-base sm:text-lg font-semibold text-left rtl:text-right text-gray-900 bg-white dark:text-white dark:bg-gray-800">
        Inspect Federation
        <p className="mt-1 text-xs sm:text-sm font-normal text-gray-500 dark:text-gray-400">
          Fetch federation info by invite code
        </p>
      </h1>

      <div className="p-4 sm:p-5 pt-0 bg-white dark:text-white dark:bg-gray-800">
        <form className="flex flex-col sm:flex-row gap-2 sm:items-center" onSubmit={handleSubmit}>
          <div className="relative flex-1 w-full">
            <input
              type="text"
              value={inviteCode}
              onChange={(e) => inspector.changeInviteCode(e.target.value)}
              disabled={checking || announcing}
              placeholder=" "
              className="block px-2.5 h-11 w-full text-sm text-gray-900 bg-gray-50 rounded-lg border-gray-300 appearance-none dark:text-white dark:bg-gray-700 dark:border-gray-600 dark:focus:border-blue-500 focus:outline-hidden focus:ring-0 focus:border-blue-600 peer border disabled:opacity-50 disabled:cursor-not-allowed"
            />
            <label className="absolute text-sm text-gray-600 dark:text-gray-400 duration-300 transform -translate-y-4 scale-75 top-2 z-10 origin-left bg-gray-50 dark:bg-gray-700 px-2 peer-focus:px-2 peer-focus:text-blue-600 dark:peer-focus:text-blue-500 peer-placeholder-shown:scale-100 peer-placeholder-shown:-translate-y-1/2 peer-placeholder-shown:top-1/2 peer-focus:top-2 peer-focus:scale-75 peer-focus:-translate-y-4 peer-focus:rtl:translate-x-1/4 peer-focus:rtl:left-auto inset-s-1">
              Invite Code
            </label>
          </div>
          <div className="flex flex-col sm:flex-row gap-2 w-full sm:w-auto">
            <button
              type="submit"
              disabled={checking}
              className="h-11 w-full sm:w-auto whitespace-nowrap font-medium rounded-lg text-sm px-5 py-2.5 text-white bg-blue-700 hover:bg-blue-800 focus:ring-4 focus:ring-blue-300 dark:bg-blue-600 dark:hover:bg-blue-700 focus:outline-hidden dark:focus:ring-blue-800 disabled:bg-blue-400 disabled:cursor-not-allowed"
            >
              {checking ? 'Checking...' : 'Check Federation'}
            </button>
            <button
              type="button"
              onClick={inspector.announce}
              disabled={!checkedInfo || announcing || announced || checking}
              title={checkedInfo ? undefined : 'Check the federation first'}
              className="h-11 w-full sm:w-auto whitespace-nowrap font-medium rounded-lg text-sm px-5 py-2.5 text-white bg-green-700 hover:bg-green-800 focus:ring-4 focus:ring-green-300 dark:bg-green-600 dark:hover:bg-green-700 focus:outline-hidden disabled:bg-green-400 disabled:cursor-not-allowed"
            >
              {announcing ? 'Announcing...' : announced ? 'Announced!' : 'Announce Federation'}
            </button>
          </div>
        </form>

        {(checking || info) && <FederationInfoTable info={info} checking={checking} />}

        {error && (
          <div className="p-3 sm:p-4 mb-4 mt-4 text-xs sm:text-sm text-red-800 rounded-lg bg-red-50 dark:bg-gray-800 dark:text-red-400 wrap-break-word">
            <span className="font-bold">Error: </span>
            {error}
          </div>
        )}

        {announced && (
          <div className="p-3 sm:p-4 mb-4 mt-4 text-xs sm:text-sm text-green-800 rounded-lg bg-green-50 dark:bg-gray-800 dark:text-green-400">
            <span className="font-bold">Success! </span>
            Federation announced successfully! Reload the page to see it listed.
          </div>
        )}
      </div>
    </ListCard>
  );
}
