import { Component, type ErrorInfo, type ReactNode } from 'react';

interface ErrorBoundaryProps {
  fallback: ReactNode;
  children: ReactNode;
}

interface ErrorBoundaryState {
  failed: boolean;
}

// Without a boundary, React unmounts the whole app on a render error, leaving a blank page
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('Render error:', error, info.componentStack);
  }

  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}

export function ReloadMessage({ message }: { message: string }) {
  return (
    <div className="flex flex-col items-center justify-center gap-3 p-6 text-center text-sm text-ink-2">
      <p>{message}</p>
      <button
        type="button"
        onClick={() => window.location.reload()}
        className="rounded-lg border border-accent/30 bg-accent-soft px-4 py-2 font-semibold text-accent-ink hover:bg-accent-soft"
      >
        Reload page
      </button>
    </div>
  );
}
