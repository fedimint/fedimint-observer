import { forwardRef, type InputHTMLAttributes } from 'react';
import { CONTROL_SIZE, FIELD, type ControlSize } from './controls';

interface TextInputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'size'> {
  size?: ControlSize;
}


export const TextInput = forwardRef<HTMLInputElement, TextInputProps>(function TextInput({ size = 'sm', className = '', ...rest }, ref) {
  return <input ref={ref} className={`${FIELD} ${CONTROL_SIZE[size]} min-w-0 ${className}`} {...rest} />;
});
