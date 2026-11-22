import { forwardRef, type ButtonHTMLAttributes } from 'react';
import { BUTTON_BASE, BUTTON_VARIANT, CONTROL_SIZE, ICON_CONTROL_SIZE, type ButtonVariant, type ControlSize } from './controls';
import { Icon, type IconName } from './Icon';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ControlSize;
  /** Leading icon; with no children the button is a square icon button (give it an aria-label) */
  icon?: IconName;
}

/** Every clickable action: one height, radius and focus ring per size */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = 'secondary', size = 'sm', icon, className = '', children, type = 'button', ...rest },
  ref,
) {
  const shape = children ? CONTROL_SIZE[size] : ICON_CONTROL_SIZE[size];
  return (
    <button ref={ref} type={type} className={`${BUTTON_BASE} ${BUTTON_VARIANT[variant]} ${shape} ${className}`} {...rest}>
      {icon && <Icon name={icon} className={size === 'sm' ? 'size-3.5' : 'size-4'} />}
      {children}
    </button>
  );
});
