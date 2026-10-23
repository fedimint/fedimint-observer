import type { ReactNode } from 'react';
import { useState } from 'react';

export type BadgeLevel = 'info' | 'warning' | 'error' | 'success';

interface BadgeProps {
  level: BadgeLevel;
  tooltip?: string;
  children: ReactNode;
  showIcon?: boolean;
}

function WarningIcon({ level }: { level: BadgeLevel }) {
  // Different colors for warning (yellow) and error (red)
  const colors = {
    warning: {
      bg: '#FEF3C7',
      stroke: '#FCD34D',
      iconStroke: '#D97706',
    },
    error: {
      bg: '#FEE2E2',
      stroke: '#FCA5A5',
      iconStroke: '#DC2626',
    },
  };

  const color = level === 'error' ? colors.error : colors.warning;

  return (
    <svg width="20" height="20" viewBox="0 0 40 40" fill="none" xmlns="http://www.w3.org/2000/svg">
      <rect width="40" height="40" rx="8" fill={color.bg} stroke={color.stroke} strokeWidth="1"/>
      <path d="M20 11L31 30H9L20 11Z" stroke={color.iconStroke} strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"/>
      <path d="M20 18V23" stroke={color.iconStroke} strokeWidth="2" strokeLinecap="round"/>
      <path d="M20 26V26.01" stroke={color.iconStroke} strokeWidth="2" strokeLinecap="round"/>
    </svg>
  );
}

export function Badge({ level, tooltip, children, showIcon }: BadgeProps) {
  const styles = {
    info: 'bg-indigo-600 text-white text-xs font-medium me-2 px-2.5 py-0.5 rounded-sm dark:bg-indigo-500',
    warning: 'bg-yellow-100 text-yellow-800 text-xs font-medium me-2 px-2.5 py-0.5 rounded-sm dark:bg-yellow-900 dark:text-yellow-300',
    error: 'bg-red-100 text-red-800 text-xs font-medium me-2 px-2.5 py-0.5 rounded-sm dark:bg-red-900 dark:text-red-300',
    success: 'bg-green-100 text-green-800 text-xs font-medium me-2 px-2.5 py-0.5 rounded-sm dark:bg-green-900 dark:text-green-300',
  };

  const [showTooltip, setShowTooltip] = useState(false);

  const tooltipStyles = {
    warning: 'bg-yellow-100 text-yellow-800 border-yellow-300 dark:bg-yellow-900 dark:text-yellow-300 dark:border-yellow-700',
    error: 'bg-red-100 text-red-800 border-red-300 dark:bg-red-900 dark:text-red-300 dark:border-red-700',
  };

  // Special handling for icon-only display with tooltip
  if (showIcon && tooltip) {
    const tooltipStyle = level === 'error' ? tooltipStyles.error : tooltipStyles.warning;
    
    return (
      <div 
        className="relative inline-block cursor-help shrink-0"
        onMouseEnter={() => setShowTooltip(true)}
        onMouseLeave={() => setShowTooltip(false)}
      >
        <WarningIcon level={level} />
        {showTooltip && (
          <div className={`absolute z-50 left-0 bottom-full mb-2 px-3 py-2 text-xs font-medium rounded-lg border whitespace-nowrap shadow-lg ${tooltipStyle}`}>
            {tooltip}
            <div className={`absolute left-2 top-full w-0 h-0 border-l-4 border-r-4 border-t-4 border-l-transparent border-r-transparent ${level === 'error' ? 'border-t-red-300 dark:border-t-red-700' : 'border-t-yellow-300 dark:border-t-yellow-700'}`}></div>
          </div>
        )}
      </div>
    );
  }

  if (tooltip) {
    return (
      <span className={styles[level]}>
        <abbr title={tooltip}>{children}</abbr>
      </span>
    );
  }

  return <span className={styles[level]}>{children}</span>;
}
