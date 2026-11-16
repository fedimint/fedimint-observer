/** The largest daily figures across all federations, so every row's chart has the same scale */
export interface ActivityScale {
  maxTransaction: number;
  /** In msats */
  maxVolume: number;
}
