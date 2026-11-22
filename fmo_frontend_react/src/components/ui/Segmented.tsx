import { useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import { CONTROL_RADIUS, type ControlSize } from './controls';

export interface SegmentOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
  /** Why the option is disabled */
  title?: string;
}

interface SegmentedProps<T extends string> {
  label: string;
  options: readonly SegmentOption<T>[];
  value: T;
  onChange: (value: T) => void;
  /** An option is about to be chosen (hovered or focused), e.g. to start loading it */
  onIntent?: (value: T) => void;
  size?: ControlSize;
}

/** A compact single-choice control (radio group), e.g. a time range; arrow keys move the choice */
export function Segmented<T extends string>({ label, options, value, onChange, onIntent, size = 'sm' }: SegmentedProps<T>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const track = useRef<HTMLDivElement>(null);
  const selectedIndex = options.findIndex((option) => option.value === value);
  const [thumb, setThumb] = useState<{ left: number; width: number } | null>(null);
  // Slides only between options, never in from the edge on first paint
  const [animate, setAnimate] = useState(false);

  useLayoutEffect(() => {
    const row = track.current;
    if (!row) return;
    const measure = () => {
      const button = refs.current[selectedIndex];
      setThumb(button ? { left: button.offsetLeft, width: button.offsetWidth } : null);
    };
    measure();
    const frame = requestAnimationFrame(() => setAnimate(true));
    const resize = new ResizeObserver(measure);
    resize.observe(row);
    return () => {
      cancelAnimationFrame(frame);
      resize.disconnect();
    };
  }, [selectedIndex, options.length]);

  const enabled = options.filter((option) => !option.disabled);

  const onKeyDown = (event: KeyboardEvent) => {
    const step = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const index = enabled.findIndex((option) => option.value === value);
    const next = enabled[(index + step + enabled.length) % enabled.length];
    onChange(next.value);
    refs.current[options.indexOf(next)]?.focus();
  };

  return (
    <div ref={track} role="radiogroup" aria-label={label} onKeyDown={onKeyDown} className={`relative inline-flex max-w-full items-stretch overflow-x-auto border border-line-strong bg-surface-2 p-0.5 scrollbar-thin ${CONTROL_RADIUS} ${size === 'sm' ? 'h-8' : 'h-10'}`}>
      {thumb && (
        <span
          aria-hidden="true"
          className={`pointer-events-none absolute inset-y-0.5 left-0 rounded-md bg-surface shadow-sm ring-1 ring-line-strong ${
            animate ? 'transition-[transform,width] duration-200 ease-out motion-reduce:transition-none' : ''
          }`}
          style={{ width: thumb.width, transform: `translateX(${thumb.left}px)` }}
        />
      )}
      {options.map((option, index) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            ref={(element) => { refs.current[index] = element; }}
            type="button"
            role="radio"
            aria-checked={selected}
            tabIndex={selected ? 0 : -1}
            disabled={option.disabled}
            title={option.title}
            onClick={() => onChange(option.value)}
            onPointerEnter={onIntent && !selected ? () => onIntent(option.value) : undefined}
            onFocus={onIntent && !selected ? () => onIntent(option.value) : undefined}
            className={`relative shrink-0 whitespace-nowrap rounded-md font-medium ${size === 'sm' ? 'px-2.5 text-xs' : 'px-3.5 text-sm'} tabular transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
              selected ? `text-ink ${thumb ? '' : 'bg-surface shadow-sm ring-1 ring-line-strong'}` : 'text-ink-3 hover:text-ink'
            }`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
