import { forwardRef, type SelectHTMLAttributes } from 'react';
import { CONTROL_SIZE, FIELD, type ControlSize } from './controls';

interface SelectProps extends Omit<SelectHTMLAttributes<HTMLSelectElement>, 'size'> {
  size?: ControlSize;
}


export const Select = forwardRef<HTMLSelectElement, SelectProps>(function Select({ size = 'sm', className = '', ...rest }, ref) {
  return <select ref={ref} className={`${FIELD} ${CONTROL_SIZE[size]} py-0 pr-8 ${className}`} {...rest} />;
});
