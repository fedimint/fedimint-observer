const MEMPOOL_PATHS: Record<string, string> = {
  bitcoin: '',
  mainnet: '',
  testnet: '/testnet',
  testnet3: '/testnet',
  testnet4: '/testnet4',
  signet: '/signet',
};

function mempoolBase(network?: string | null): string | null {
  const path = MEMPOOL_PATHS[network?.toLowerCase() ?? ''];
  return path === undefined ? null : `https://mempool.space${path}`;
}

/** Explorer page of a transaction (or the transaction of a `txid:vout` outpoint) */
export function explorerTxUrl(txidOrOutPoint: string, network?: string | null): string | null {
  const base = mempoolBase(network);
  return base && `${base}/tx/${txidOrOutPoint.split(':')[0]}`;
}

export function explorerAddressUrl(address: string, network?: string | null): string | null {
  const base = mempoolBase(network);
  return base && `${base}/address/${address}`;
}


export function nostrNetworkTag(network?: string | null): string | null {
  const name = network?.trim().toLowerCase();
  if (!name || name === 'unknown') return null;
  return name === 'bitcoin' ? 'mainnet' : name;
}
