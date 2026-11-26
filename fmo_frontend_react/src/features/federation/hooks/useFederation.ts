import { useOutletContext } from 'react-router-dom';
import type { useFederationDetails } from './useFederationDetails';

type Resources = ReturnType<typeof useFederationDetails>;
type Details = NonNullable<Resources['summary']['data']>;


export interface FederationContext extends Details {
  id: string;
  config: Resources['config']['data'];
}

/** The federation whose section is shown, loaded once by `FederationLayout` */
export function useFederation(): FederationContext {
  return useOutletContext<FederationContext>();
}
