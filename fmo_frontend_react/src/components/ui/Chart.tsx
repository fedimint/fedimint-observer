import { lazy, Suspense, useEffect, useRef, useState, type ComponentType } from 'react';
import type { ChartCanvasProps } from './ChartCanvas';

const FEATURES = {
  markLine: () => import('./chartFeatures/markLine'),
  dataZoom: () => import('./chartFeatures/dataZoom'),
};
export type ChartFeature = keyof typeof FEATURES;

const canvases = new Map<string, ComponentType<ChartCanvasProps>>();

/** The chart library with just the extra features a chart asks for, loaded once per set */
function canvasWith(features: readonly ChartFeature[]) {
  const key = [...features].sort().join();
  let canvas = canvases.get(key);
  if (!canvas) {
    canvas = lazy(() => Promise.all([import('./ChartCanvas'), ...features.map((feature) => FEATURES[feature]())])
      .then(([module]) => ({ default: module.ChartCanvas })));
    canvases.set(key, canvas);
  }
  return canvas;
}

/** Whether the element has entered the viewport; it stays true once it has */
function useInViewport<T extends Element>() {
  const ref = useRef<T>(null);
  const [visible, setVisible] = useState(() => typeof IntersectionObserver === 'undefined');
  useEffect(() => {
    const element = ref.current;
    if (visible || !element) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) setVisible(true);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [visible]);
  return [ref, visible] as const;
}

/** Loads and draws the chart library only once the chart enters the screen, so the page's text and data come first */
export function Chart({ label, features = [], ...canvas }: ChartCanvasProps & { label?: string; features?: readonly ChartFeature[] }) {
  const [ref, visible] = useInViewport<HTMLDivElement>();
  const Canvas = canvasWith(features);
  const placeholder = <div role="status" aria-label="Loading chart" className="skeleton w-full rounded" style={{ height: canvas.height }} />;
  return (
    <div ref={ref} role={label ? 'img' : undefined} aria-label={label}>
      {visible ? <Suspense fallback={placeholder}><Canvas {...canvas} /></Suspense> : placeholder}
    </div>
  );
}
