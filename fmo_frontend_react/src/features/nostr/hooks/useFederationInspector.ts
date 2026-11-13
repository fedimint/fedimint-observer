import { useEffect, useState } from 'react';
import { api } from '../../../services/api';
import { publishNostrEvent } from '../../../services/nostr';
import { errorMessage } from '../../../utils/errors';
import { parseConfig, type FederationConfig } from '../../../utils/config';
import { nostrNetworkTag } from '../../../utils/network';
import { federationName } from '../lib/federationNames';

export interface FederationInfo extends FederationConfig {
  name: string;
  /** The invite code this info was fetched for; an announcement is built from it alone */
  invite: string;
}

/**
 * Checks a federation by invite code and announces it on Nostr. A `check`
 * query parameter is filled in and checked on arrival, so pages can deep-link here.
 */
export function useFederationInspector() {
  const [inviteCode, setInviteCode] = useState('');
  const [checking, setChecking] = useState(false);
  const [info, setInfo] = useState<FederationInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [announcing, setAnnouncing] = useState(false);
  const [announced, setAnnounced] = useState(false);
  // Announcing is only possible for the invite code that was checked, unchanged
  const checkedInfo = info && info.invite === inviteCode ? info : null;

  const changeInviteCode = (value: string) => {
    setInviteCode(value);
    // What was checked described the previous invite code
    setInfo(null);
    setAnnounced(false);
    setError(null);
  };

  const check = async (invite: string, signal?: AbortSignal) => {
    setInfo(null);
    setError(null);
    setAnnounced(false);

    if (!invite.trim()) {
      setError('Invite code not present. Please enter a valid invite code.');
      return;
    }

    setChecking(true);

    try {
      const config = await api.getFederationConfigByInvite(invite, signal);

      // The metadata request runs after the config request so the server answers it
      // from the config it just cached. Without it the name is unknown.
      const meta = await api.getFederationMetaByInvite(invite, signal).catch(() => null);
      if (signal?.aborted) return;

      setInfo({
        name: (meta && federationName(meta)) || 'Unknown',
        ...parseConfig(config),
        invite,
      });
    } catch (err) {
      if (signal?.aborted) return;
      setError(`Could not check the federation: ${errorMessage(err)}`);
    } finally {
      if (!signal?.aborted) setChecking(false);
    }
  };

  // Deep link: fill in and check the invite code from the `check` parameter
  useEffect(() => {
    const checkParam = new URLSearchParams(window.location.search).get('check');
    if (!checkParam) return;

    const controller = new AbortController();
    setInviteCode(checkParam);
    check(checkParam, controller.signal);
    // Only on arrival; later URL changes don't re-check
    return () => controller.abort();
  }, []);

  const announce = async () => {
    // Every tag comes from the one checked invite code, never from later edits
    const checked = checkedInfo;
    if (!checked) return;

    setAnnouncing(true);
    setError(null);
    setAnnounced(false);

    try {
      await publishNostrEvent(async () => {
        // The server derives the federation ID from the invite code, since calculating
        // it client-side would require crypto libraries
        const federationId = await api.getFederationIdByInvite(checked.invite);

        if (!checked.invite.trim()) {
          throw new Error('Invite code is required');
        }
        // Without a known network the tag is left out rather than guessed
        const network = nostrNetworkTag(checked.network);

        return {
          kind: 38173,
          tags: [
            ['d', federationId],
            ['u', checked.invite],
            ...(network ? [['n', network]] : []),
            ['modules', checked.modules.join(',')],
          ],
          content: JSON.stringify(checked.raw.global?.meta || {}),
        };
      }, (event) => api.publishNostrFederation(event));

      setAnnounced(true);
      setError(null);
    } catch (err) {
      setError(`Could not announce the federation: ${errorMessage(err)}`);
      setAnnounced(false);
    } finally {
      setAnnouncing(false);
    }
  };

  return {
    inviteCode,
    changeInviteCode,
    checking,
    info,
    checkedInfo,
    error,
    announcing,
    announced,
    check: () => check(inviteCode),
    announce,
  };
}
