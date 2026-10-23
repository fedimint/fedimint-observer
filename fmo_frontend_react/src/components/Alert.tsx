export type AlertLevel = 'info' | 'warning' | 'error' | 'success';

interface AlertProps {
  message: string;
  level: AlertLevel;
}

export function Alert({ message, level }: AlertProps) {
  const styles = {
    info: 'p-4 mb-4 text-sm text-blue-800 rounded-lg bg-blue-50 dark:bg-gray-800 dark:text-blue-400',
    warning: 'p-4 mb-4 text-sm text-yellow-800 rounded-lg bg-yellow-50 dark:bg-gray-800 dark:text-yellow-300',
    error: 'p-4 mb-4 text-sm text-red-800 rounded-lg bg-red-50 dark:bg-gray-800 dark:text-red-400',
    success: 'p-4 mb-4 text-sm text-green-800 rounded-lg bg-green-50 dark:bg-gray-800 dark:text-green-400',
  };

  const titles = {
    info: 'Info: ',
    warning: 'Warning: ',
    error: 'Error: ',
    success: 'Success: ',
  };

  return (
    <div className={styles[level]} role="alert">
      <span className="font-bold">{titles[level]}</span>
      {message}
    </div>
  );
}
