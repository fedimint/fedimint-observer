import { useOutletContext } from 'react-router-dom';
import type { useFederationDetails } from './useFederationDetails';

type Details = NonNullable<ReturnType<typeof useFederationDetails>['data']>;

/** What every federation section gets from the layout that keeps its header */
export interface FederationContext extends Details {
  id: string;
}

/** The federation whose section is shown, loaded once by `FederationLayout` */
export function useFederation(): FederationContext {
  return useOutletContext<FederationContext>();
}
