import { useEffect, useState } from 'react';
import { api } from '../../../services/api';
import { publishNostrEvent } from '../../../services/nostr';
import { errorMessage } from '../../../utils/errors';
import { parseConfig, type FederationConfig } from '../../../utils/config';
import { nostrNetworkTag } from '../../../utils/network';
import { federationName } from '../lib/federationNames';

export interface FederationInfo extends FederationConfig {
  name: string;

  invite: string;
}


export function useFederationInspector() {
  const [inviteCode, setInviteCode] = useState('');
  const [checking, setChecking] = useState(false);
  const [info, setInfo] = useState<FederationInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [announcing, setAnnouncing] = useState(false);
  const [announced, setAnnounced] = useState(false);
  const checkedInfo = info && info.invite === inviteCode ? info : null;

  const changeInviteCode = (value: string) => {
    setInviteCode(value);
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

  useEffect(() => {
    const checkParam = new URLSearchParams(window.location.search).get('check');
    if (!checkParam) return;

    const controller = new AbortController();
    setInviteCode(checkParam);
    check(checkParam, controller.signal);
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
        const federationId = await api.getFederationIdByInvite(checked.invite);

        if (!checked.invite.trim()) {
          throw new Error('Invite code is required');
        }
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
