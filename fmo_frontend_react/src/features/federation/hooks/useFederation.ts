import { useOutletContext } from 'react-router-dom';
import type { useFederationDetails } from './useFederationDetails';
import type { federationOutline } from '../../../utils/config';

type Resources = ReturnType<typeof useFederationDetails>;
type Details = NonNullable<Resources['summary']['data']>;


export interface FederationContext extends Details, ReturnType<typeof federationOutline> {
  id: string;
  config: Resources['config']['data'];
}

/** The federation whose section is shown, loaded once by `FederationLayout` */
export function useFederation(): FederationContext {
  return useOutletContext<FederationContext>();
}
