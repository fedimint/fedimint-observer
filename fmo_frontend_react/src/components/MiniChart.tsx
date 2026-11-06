interface CombinedMiniChartProps {
  transactionData: number[];
  volumeData: number[];
  dates?: string[];
  formatTransaction?: (value: number) => string;
  formatVolume?: (value: number) => string;
  maxTransaction?: number;
  maxVolume?: number;
}

// Fourteen bars need no canvas engine, per-row observers or animation loop.
export function CombinedMiniChart({ transactionData, volumeData, dates,
  formatTransaction = String, formatVolume = String, maxTransaction, maxVolume }: CombinedMiniChartProps) {
  const transactions = transactionData.slice(-7);
  const volumes = volumeData.slice(-7);
  const labels = dates?.slice(-7);
  const transactionMax = Math.max(1, maxTransaction ?? Math.max(...transactions));
  const volumeMax = Math.max(1, maxVolume ?? Math.max(...volumes));
  const transactionAverage = transactions.reduce((sum, value) => sum + value, 0) / (transactions.length || 1);
  const volumeAverage = volumes.reduce((sum, value) => sum + value, 0) / (volumes.length || 1);
  const average = `7-day average: ${formatTransaction(transactionAverage)} transactions; ${formatVolume(volumeAverage)} volume`;
  return <div role="img" aria-label={`Transactions in blue and volume in green. ${average}`} className="flex h-10 w-full items-end gap-1 md:h-14 xl:h-16">
    {transactions.map((count, index) => {
      const volume = volumes[index] ?? 0;
      return <div key={index} className="flex h-full min-w-0 flex-1 items-end justify-center gap-0.5"
        title={`${labels?.[index] ?? `Day ${index + 1}`}\nTransactions: ${formatTransaction(count)}\nVolume: ${formatVolume(volume)}\n${average}`}>
        <span className={`w-[30%] rounded-t-[2px] ${count === 0 ? 'opacity-20' : ''}`} style={{ backgroundColor: '#3b82f6', height: `${Math.min(100, count / transactionMax * 100)}%`, minHeight: 1 }} />
        <span className={`w-[30%] rounded-t-[2px] ${volume === 0 ? 'opacity-20' : ''}`} style={{ backgroundColor: '#10b981', height: `${Math.min(100, volume / volumeMax * 100)}%`, minHeight: 1 }} />
      </div>;
    })}
  </div>;
}
