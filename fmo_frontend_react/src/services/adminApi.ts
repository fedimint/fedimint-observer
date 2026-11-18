import type { AdminAuditPage, AdminDelegate, AdminKeys, AdminSession, GrantAdminKey } from '../types/api';
import { path } from './api';
import { request, requestNoContent, type HttpMethod } from './http';
import { absoluteUrl, authorizationHeader, type NostrSigner } from './nostrAuth';

/** The key admin requests are signed with, and what signs them */
export interface AdminIdentity {
  signer: NostrSigner;
  /** 64 hex characters */
  pubkey: string;
}

// Every admin request is signed for itself, so each asks the signer once. The
// body is serialized here, once: the bytes signed are the bytes sent.
async function signed(identity: AdminIdentity, method: HttpMethod, url: string, json?: unknown) {
  const absolute = absoluteUrl(url);
  const body = json === undefined ? undefined : JSON.stringify(json);
  const authorization = await authorizationHeader(identity.signer, identity.pubkey, { url: absolute, method, body });
  return { url: absolute, options: { method, body, headers: { Authorization: authorization }, noStore: true } };
}

async function send<T>(identity: AdminIdentity, method: HttpMethod, url: string, json?: unknown, signal?: AbortSignal): Promise<T> {
  const { url: absolute, options } = await signed(identity, method, url, json);
  signal?.throwIfAborted();
  return request<T>(absolute, { ...options, signal });
}

// Failures reject with an ApiError (see ./http) or a NostrSignerError, both with a message fit to show
export const adminApi = {
  /** Who the key is to this observer; 403 if it has no access */
  session: (identity: AdminIdentity, signal?: AbortSignal) =>
    send<AdminSession>(identity, 'GET', path`/admin/session`, undefined, signal),

  /** Starts observing the invite's federation and answers its id */
  addFederation: (identity: AdminIdentity, invite: string, signal?: AbortSignal) =>
    send<string>(identity, 'PUT', path`/federations`, { invite }, signal),

  listKeys: (identity: AdminIdentity, signal?: AbortSignal) =>
    send<AdminKeys>(identity, 'GET', path`/admin/keys`, undefined, signal),

  /** `key` as npub1… or hex; granting a key again replaces its label and expiry */
  grantKey: (identity: AdminIdentity, key: string, grant: GrantAdminKey, signal?: AbortSignal) =>
    send<AdminDelegate>(identity, 'PUT', path`/admin/keys/${key}`, grant, signal),

  async revokeKey(identity: AdminIdentity, pubkey: string, signal?: AbortSignal): Promise<void> {
    const { url, options } = await signed(identity, 'DELETE', path`/admin/keys/${pubkey}`);
    signal?.throwIfAborted();
    await requestNoContent(url, { ...options, signal });
  },

  /** Newest first; `after` is the previous page's `next` */
  auditLog(identity: AdminIdentity, after?: number, signal?: AbortSignal) {
    const query = new URLSearchParams({ limit: '25' });
    if (after !== undefined) query.set('after', String(after));
    return send<AdminAuditPage>(identity, 'GET', `${path`/admin/audit`}?${query}`, undefined, signal);
  },
};
