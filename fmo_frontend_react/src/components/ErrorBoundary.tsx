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
    <div className="flex flex-col items-center justify-center gap-3 p-6 text-center text-sm text-gray-600 dark:text-gray-300">
      <p>{message}</p>
      <button
        type="button"
        onClick={() => window.location.reload()}
        className="rounded-lg border border-blue-300 bg-blue-50 px-4 py-2 font-semibold text-blue-800 hover:bg-blue-100 dark:border-blue-800 dark:bg-blue-950/50 dark:text-blue-200"
      >
        Reload page
      </button>
    </div>
  );
}
