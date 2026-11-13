/** An inline "Try again" link-style button inside an error message */
export function RetryButton({ onClick, label = 'Try again' }: { onClick: () => void; label?: string }) {
  return (
    <button type="button" onClick={onClick} className="font-medium underline hover:no-underline">
      {label}
    </button>
  );
}
